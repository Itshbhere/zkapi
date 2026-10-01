package config

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestDefaultDirectoryPreservesLegacyZKAPIWallet(t *testing.T) {
	base := t.TempDir()
	next := filepath.Join(base, "zkapi-clientd")
	if got, err := defaultDirAt(base); err != nil || got != next {
		t.Fatalf("fresh default %s: %v", got, err)
	}
	legacy := filepath.Join(base, "oa-chat")
	c, _ := Default()
	c.OrgURL = "https://org.openanonymity.ai"
	if err := Init(legacy, c); err != nil {
		t.Fatal(err)
	}
	before, _ := os.ReadFile(filepath.Join(legacy, "config.json"))
	if err := os.WriteFile(filepath.Join(legacy, "tickets.json"), []byte("preserve legacy artifacts"), 0600); err != nil {
		t.Fatal(err)
	}
	if got, err := defaultDirAt(base); err != nil || got != legacy {
		t.Fatalf("legacy default %s: %v", got, err)
	}
	after, _ := os.ReadFile(filepath.Join(legacy, "config.json"))
	if string(before) != string(after) {
		t.Fatal("wallet profile changed during selection")
	}
	if _, err := os.Stat(filepath.Join(legacy, "management-token")); !os.IsNotExist(err) {
		t.Fatal("directory lookup created credentials")
	}
	if _, err := os.Stat(next); !os.IsNotExist(err) {
		t.Fatal("legacy selection created a new profile")
	}
	if err := os.Mkdir(next, 0700); err != nil {
		t.Fatal(err)
	}
	if got, err := defaultDirAt(base); err != nil || got != next {
		t.Fatalf("existing renamed directory lost priority: %s %v", got, err)
	}
}

func TestDefaultDirectoryRejectsIncompatibleLegacyState(t *testing.T) {
	for _, kind := range []string{"ticket", "invalid", "orphan", "symlink"} {
		t.Run(kind, func(t *testing.T) {
			base := t.TempDir()
			legacy := filepath.Join(base, "oa-chat")
			if kind == "symlink" {
				if err := os.Symlink(t.TempDir(), legacy); err != nil {
					t.Fatal(err)
				}
			} else {
				if err := os.Mkdir(legacy, 0700); err != nil {
					t.Fatal(err)
				}
				name, data := "config.json", []byte("invalid")
				if kind == "ticket" {
					c, _ := Default()
					c.Backend = "ticket"
					data, _ = json.Marshal(c)
				}
				if kind == "orphan" {
					name = "tickets.json"
				}
				if err := os.WriteFile(filepath.Join(legacy, name), data, 0600); err != nil {
					t.Fatal(err)
				}
			}
			if _, err := defaultDirAt(base); err == nil {
				t.Fatal("silently replaced legacy wallet")
			}
			if _, err := os.Stat(filepath.Join(base, "zkapi-clientd")); !os.IsNotExist(err) {
				t.Fatal("created new wallet")
			}
		})
	}
}

func TestDefaultDirectoryEnvironmentPrecedence(t *testing.T) {
	old := filepath.Join(t.TempDir(), "legacy-override")
	next := filepath.Join(t.TempDir(), "new-override")
	t.Setenv("OA_CHAT_CONFIG_DIR", old)
	t.Setenv("ZKAPI_CLIENTD_CONFIG_DIR", "")
	if got, err := DefaultDir(); err != nil || got != old {
		t.Fatalf("legacy override: %s %v", got, err)
	}
	t.Setenv("ZKAPI_CLIENTD_CONFIG_DIR", next)
	if got, err := DefaultDir(); err != nil || got != next {
		t.Fatalf("new override: %s %v", got, err)
	}
}

func TestExplicitLegacyTicketProfileFailsWithoutMutation(t *testing.T) {
	dir := filepath.Join(t.TempDir(), "profile")
	if err := os.Mkdir(dir, 0700); err != nil {
		t.Fatal(err)
	}
	c, _ := Default()
	c.Backend = "ticket"
	data, _ := json.Marshal(c)
	if err := os.WriteFile(filepath.Join(dir, "config.json"), data, 0600); err != nil {
		t.Fatal(err)
	}
	if _, err := Load(dir); err == nil || !strings.Contains(err.Error(), "requires a zkAPI native ETH profile") {
		t.Fatalf("ticket migration: %v", err)
	}
	after, _ := os.ReadFile(filepath.Join(dir, "config.json"))
	if string(data) != string(after) {
		t.Fatal("ticket profile rewritten")
	}
	if _, err := os.Stat(filepath.Join(dir, "management-token")); !os.IsNotExist(err) {
		t.Fatal("ticket profile acquired new credentials")
	}
}
