package main

import (
	"bytes"
	"context"
	"io"
	"log"
	"net"
	"net/http"
	"net/http/httptest"
	"os"
	"os/exec"
	"os/signal"
	"path/filepath"
	"strings"
	"sync"
	"syscall"
	"testing"
	"time"

	"github.com/OpenAnonymity/zkapi/zkapi-clientd/internal/config"
)

type serveOutput struct {
	mu      sync.Mutex
	buffer  bytes.Buffer
	changed chan struct{}
}

func (s *serveOutput) Write(p []byte) (int, error) {
	s.mu.Lock()
	n, err := s.buffer.Write(p)
	s.mu.Unlock()
	select {
	case s.changed <- struct{}{}:
	default:
	}
	return n, err
}

func (s *serveOutput) String() string {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.buffer.String()
}

func TestServeCommandWritesStatusAndLogsToStdout(t *testing.T) {
	if os.Getenv("ZKAPI_CLIENTD_SERVE_STDOUT_TEST_HELPER") == "1" {
		dir := os.Getenv("ZKAPI_CLIENTD_SERVE_STDOUT_TEST_DIR")
		c, err := config.Load(dir)
		if err != nil {
			t.Fatal(err)
		}
		expected := c
		ctx, cancel := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
		defer cancel()
		if err := serveSnapshot(ctx, dir, c, expected, os.Stdout); err != nil {
			log.Print(err)
			os.Exit(1)
		}
		os.Exit(0)
	}
	c, err := config.Default()
	if err != nil {
		t.Fatal(err)
	}
	// Exercise the foreground lifecycle with an isolated external companion.
	bridge := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Header.Get("Authorization") != "Bearer "+c.ZKAPI.BridgeToken {
			w.WriteHeader(401)
			return
		}
		switch r.URL.Path {
		case "/v1/session-events":
			io.WriteString(w, `{"events":[],"next_sequence":0}`)
		default:
			io.WriteString(w, `{"has_note":true}`)
		}
	}))
	defer bridge.Close()
	c.ZKAPI.ExternalCompanion, c.ZKAPI.ClientURL = true, bridge.URL
	c.RequireAPIKey = true // Exercise explicit authentication while the default is keyless.
	c.ZKAPI.Binary = filepath.Join(t.TempDir(), "missing-companion")
	c.ZKAPI.ProofSetupDir = filepath.Join(t.TempDir(), "missing-proof-assets")
	// Configuration requires a nonzero port; release a local ephemeral port just
	// before starting the child. No configured remote service is contacted.
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	c.Listen = listener.Addr().String()
	dir := filepath.Join(t.TempDir(), "config")
	if err := config.Init(dir, c); err != nil {
		listener.Close()
		t.Fatal(err)
	}
	savedConfig, err := os.ReadFile(filepath.Join(dir, "config.json"))
	if err != nil {
		t.Fatal(err)
	}
	listener.Close()
	executable, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	cmd := exec.Command(executable, "-test.run=^TestServeCommandWritesStatusAndLogsToStdout$")
	cmd.Env = append(os.Environ(), "ZKAPI_CLIENTD_SERVE_STDOUT_TEST_HELPER=1", "ZKAPI_CLIENTD_SERVE_STDOUT_TEST_DIR="+dir)
	stdout := &serveOutput{changed: make(chan struct{}, 1)}
	var stderr bytes.Buffer
	cmd.Stdout, cmd.Stderr = stdout, &stderr
	if err := cmd.Start(); err != nil {
		t.Fatal(err)
	}
	done := make(chan struct{})
	var waitErr error
	go func() {
		waitErr = cmd.Wait()
		close(done)
	}()
	defer func() {
		_ = cmd.Process.Kill()
		<-done
	}()
	readyDeadline := time.NewTimer(10 * time.Second)
	defer readyDeadline.Stop()
	for !strings.Contains(stdout.String(), "Localhost inference needs no API key") && !strings.Contains(stdout.String(), "Use zkapi-clientd config --api-key") {
		select {
		case <-stdout.changed:
		case <-done:
			t.Fatalf("serve stopped before readiness: %v; stderr: %s", waitErr, stderr.String())
		case <-readyDeadline.C:
			t.Fatalf("serve did not report readiness on stdout: %s", stdout.String())
		}
	}
	client := &http.Client{Timeout: 5 * time.Second, Transport: &http.Transport{Proxy: nil}}
	defer client.CloseIdleConnections()
	active, err := resolveActiveConfig(context.Background(), c)
	if err != nil || active.Backend != "zkapi" || active.ZKAPI != c.ZKAPI {
		t.Fatalf("running mode did not override saved default safely: %v", err)
	}
	req, err := http.NewRequest(http.MethodGet, "http://"+c.Listen+"/v1/models?secret-query", nil)
	if err != nil {
		t.Fatal(err)
	}
	req.Header.Set("Authorization", "Bearer secret-client-token")
	resp, err := client.Do(req)
	if err != nil {
		t.Fatal(err)
	}
	io.Copy(io.Discard, resp.Body)
	resp.Body.Close()
	if resp.StatusCode != http.StatusUnauthorized {
		t.Fatalf("unauthenticated request status = %d", resp.StatusCode)
	}
	if err := cmd.Process.Signal(syscall.SIGTERM); err != nil {
		t.Fatal(err)
	}
	select {
	case <-done:
		if waitErr != nil {
			t.Fatalf("serve failed: %v; stderr: %s", waitErr, stderr.String())
		}
	case <-time.After(10 * time.Second):
		t.Fatal("serve did not shut down after SIGTERM")
	}
	for _, want := range []string{"Starting local API service", "listening at http://" + c.Listen + "/v1", "direct HTTPS (network proxy off)", "/v1/models", "401", "Stopping local API service", "Local API service stopped"} {
		if !strings.Contains(stdout.String(), want) {
			t.Errorf("stdout does not contain %q: %s", want, stdout.String())
		}
	}
	if stderr.Len() != 0 {
		t.Errorf("successful serve wrote to stderr: %s", stderr.String())
	}
	afterConfig, err := os.ReadFile(filepath.Join(dir, "config.json"))
	if err != nil || !bytes.Equal(savedConfig, afterConfig) {
		t.Fatal("serve override changed the saved wallet configuration", err)
	}

}
