package zkapi

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"net/http"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
)

func TestNativeDeploymentManifestsMatchReviewedBrowserProfiles(t *testing.T) {
	for _, network := range []string{"mainnet", "sepolia"} {
		t.Run(network, func(t *testing.T) {
			identity, raw, err := pinnedDeployment(network)
			if err != nil {
				t.Fatal(err)
			}
			var manifest map[string]any
			if err := json.Unmarshal(raw, &manifest); err != nil {
				t.Fatal(err)
			}
			profileRaw, err := os.ReadFile(filepath.Join("testdata", "browser-profiles", network+".json"))
			if err != nil {
				t.Fatal(err)
			}
			var profile struct {
				Trusted     map[string]any `json:"trusted_deployment"`
				ManifestURL string         `json:"deployment_manifest_url"`
			}
			if err := json.Unmarshal(profileRaw, &profile); err != nil {
				t.Fatal(err)
			}
			for _, field := range []string{"deployment_id", "chain_id", "contract_address", "billing_asset", "billing_unit", "billing_token_address", "native_asset_wei_per_unit", "native_price_feed_address", "native_price_feed_decimals", "native_price_max_age_seconds", "protocol_server_url", "indexer_url", "rpc_url", "request_charge_cap", "state_signing_key", "clearance_signing_key"} {
				if !reflect.DeepEqual(manifest[field], profile.Trusted[field]) {
					t.Errorf("%s differs from reviewed browser profile", field)
				}
			}
			proof := manifest["proof_setup"].(map[string]any)
			for _, field := range []string{"circuit_id", "request_proving_key_sha256", "withdrawal_proving_key_sha256"} {
				if proof[field] != profile.Trusted[field] {
					t.Errorf("%s differs from reviewed browser proof pin", field)
				}
			}
			privacy := manifest["privacy_mode"].(map[string]any)
			if privacy["verifier_url"] != profile.Trusted["verifier_url"] || privacy["openrouter_inference_base"] != profile.Trusted["openrouter_inference_base"] || privacy["ephemeral_key_source"] != "oa_org" {
				t.Fatal("deployment changed OA issuer/verifier boundary")
			}
			expectedURL, err := Manifest(network)
			if err != nil || expectedURL != profile.ManifestURL || manifest["config_url"] != expectedURL {
				t.Fatal("deployment manifest URL does not match reviewed origin")
			}
			if identity.Asset != "native_eth" || identity.Unit != "gwei" || identity.Proof.Circuit != "zkapi-v2-note-bound-v1" {
				t.Fatal("wrong native deployment identity")
			}
		})
	}
	mainnet, mainnetRaw, err := pinnedDeployment("mainnet")
	if err != nil {
		t.Fatal(err)
	}
	fallback, fallbackRaw, err := pinnedDeployment("")
	if err != nil || mainnet != fallback || string(mainnetRaw) != string(fallbackRaw) {
		t.Fatal("empty network stopped selecting mainnet")
	}
	if _, _, err := pinnedDeployment("../sepolia"); err == nil {
		t.Fatal("unsupported network path accepted")
	}
}

func TestDeploymentManifestPersistenceIsPrivateAndNeverRebinds(t *testing.T) {
	_, raw, err := pinnedDeployment("sepolia")
	if err != nil {
		t.Fatal(err)
	}
	dir := t.TempDir()
	path, err := writeDeploymentManifest(dir, raw)
	if err != nil {
		t.Fatal(err)
	}
	info, err := os.Lstat(path)
	if err != nil || !info.Mode().IsRegular() || info.Mode().Perm() != 0600 {
		t.Fatal("manifest is not a private regular file")
	}
	before, err := os.ReadFile(path)
	if err != nil || string(before) != string(raw) {
		t.Fatal("saved manifest differs from pin")
	}
	if again, err := writeDeploymentManifest(dir, raw); err != nil || again != path {
		t.Fatalf("same deployment did not resume: %v", err)
	}
	_, other, err := pinnedDeployment("mainnet")
	if err != nil {
		t.Fatal(err)
	}
	if _, err := writeDeploymentManifest(dir, other); err == nil {
		t.Fatal("existing manifest silently rebound")
	}
	after, _ := os.ReadFile(path)
	if string(before) != string(after) {
		t.Fatal("mismatch overwrote existing manifest")
	}
}

func TestDeploymentManifestRejectsSymlinksAndUnsafeFiles(t *testing.T) {
	_, raw, err := pinnedDeployment("sepolia")
	if err != nil {
		t.Fatal(err)
	}
	for _, kind := range []string{"symlink", "public", "directory", "malformed"} {
		t.Run(kind, func(t *testing.T) {
			dir := t.TempDir()
			path := filepath.Join(dir, "deployment-manifest.json")
			target := filepath.Join(dir, "original.json")
			switch kind {
			case "symlink":
				if err := os.WriteFile(target, raw, 0600); err != nil {
					t.Fatal(err)
				}
				if err := os.Symlink(target, path); err != nil {
					t.Fatal(err)
				}
			case "public":
				if err := os.WriteFile(path, raw, 0644); err != nil {
					t.Fatal(err)
				}
				if err := os.Chmod(path, 0644); err != nil {
					t.Fatal(err)
				}
			case "directory":
				if err := os.Mkdir(path, 0700); err != nil {
					t.Fatal(err)
				}
			case "malformed":
				if err := os.WriteFile(path, []byte(`{"deployment_id":"unrelated"}`), 0600); err != nil {
					t.Fatal(err)
				}
			}
			if _, err := writeDeploymentManifest(dir, raw); err == nil {
				t.Fatalf("accepted %s manifest", kind)
			}
			if kind == "symlink" {
				info, err := os.Lstat(path)
				contents, readErr := os.ReadFile(target)
				if err != nil || info.Mode()&os.ModeSymlink == 0 || readErr != nil || string(contents) != string(raw) {
					t.Fatal("refusal modified symlink or target")
				}
			}
			if kind == "malformed" {
				contents, _ := os.ReadFile(path)
				if !strings.Contains(string(contents), "unrelated") {
					t.Fatal("refusal overwrote recovery manifest")
				}
			}
		})
	}
}

func TestManifestMigrationRequiresBothCompleteManifests(t *testing.T) {
	_, packaged, err := pinnedDeployment("sepolia")
	if err != nil {
		t.Fatal(err)
	}
	if digest := sha256.Sum256(packaged); hex.EncodeToString(digest[:]) != canonicalSepoliaManifestSHA256 {
		t.Fatal("packaged Sepolia manifest changed; review both migration pins before updating")
	}
	// Synthetic endpoints exercise the same exact-byte guard without retaining
	// a retired service address in fixtures or contacting it during tests.
	previous := bytes.ReplaceAll(packaged, []byte("https://zkapi-sepolia.openanonymity.ai"), []byte("https://retired.example"))
	sourceHash, targetHash := sha256.Sum256(previous), sha256.Sum256(packaged)
	sourcePin, targetPin := hex.EncodeToString(sourceHash[:]), hex.EncodeToString(targetHash[:])
	if !matchesManifestMigration(previous, packaged, sourcePin, targetPin) {
		t.Fatal("exact source and target did not match")
	}
	if isSepoliaOriginMigration(previous, packaged) {
		t.Fatal("synthetic manifest used the production migration exception")
	}
	for _, change := range []struct{ name, before, after string }{
		{"vault", "0x49fA19f9bdECe7A48Ebc7749fD69aD40F577590F", "0x1111111111111111111111111111111111111111"},
		{"deployment", "fresh-20260930", "fresh-20260928"},
		{"signer", "0x25a4453190e930f9716eebcab165170706d31d1b3969d106a50d7ae374a23d61", "0x1"},
		{"proof", "c894b261a13f571d0df36be29734aabf2a8cd7162baddc5e08a50341aa076584", strings.Repeat("a", 64)},
		{"issuer", "https://org-staging.openanonymity.ai", "https://untrusted.example"},
		{"additional field", "{\n", "{\n  \"unreviewed\": true,\n"},
		{"formatting", "{\n", "{\n\n"},
	} {
		t.Run(change.name, func(t *testing.T) {
			changedSource := bytes.Replace(previous, []byte(change.before), []byte(change.after), 1)
			changedTarget := bytes.Replace(packaged, []byte(change.before), []byte(change.after), 1)
			if bytes.Equal(changedSource, previous) || bytes.Equal(changedTarget, packaged) {
				t.Fatal("test did not mutate both manifests")
			}
			if matchesManifestMigration(changedSource, packaged, sourcePin, targetPin) ||
				matchesManifestMigration(previous, changedTarget, sourcePin, targetPin) ||
				matchesManifestMigration(changedSource, changedTarget, sourcePin, targetPin) {
				t.Fatal("changed source or target accepted")
			}
		})
	}
	otherOrigin := bytes.ReplaceAll(previous, []byte("https://retired.example"), []byte("https://untrusted.example"))
	partial := bytes.Replace(previous, []byte("https://retired.example"), []byte("https://zkapi-sepolia.openanonymity.ai"), 1)
	for _, candidate := range [][]byte{otherOrigin, partial, packaged, nil} {
		if matchesManifestMigration(candidate, packaged, sourcePin, targetPin) {
			t.Fatal("unexpected source origin or missing source accepted")
		}
	}
	_, mainnet, _ := pinnedDeployment("mainnet")
	if matchesManifestMigration(previous, mainnet, sourcePin, targetPin) ||
		matchesManifestMigration(previous, nil, sourcePin, targetPin) ||
		matchesManifestMigration(packaged, previous, sourcePin, targetPin) {
		t.Fatal("unrelated, missing or reversed replacement accepted")
	}
}

// Optional integration fixture: extract the immutable prior release manifest
// with the command in CLI_PACKAGING.md. Default tests remain offline; reviewers
// can verify the complete persisted-wallet path using the actual prior bytes.
func TestSepoliaOriginMigrationPreservesWalletAndRejectsOtherChanges(t *testing.T) {
	_, packaged, err := pinnedDeployment("sepolia")
	if err != nil {
		t.Fatal(err)
	}
	fixture := os.Getenv("ZKAPI_TEST_PREVIOUS_MANIFEST")
	if fixture == "" {
		t.Skip("set ZKAPI_TEST_PREVIOUS_MANIFEST to test the actual prior release manifest")
	}
	legacy, err := os.ReadFile(fixture)
	if err != nil {
		t.Fatal(err)
	}
	if digest := sha256.Sum256(legacy); hex.EncodeToString(digest[:]) != previousSepoliaManifestSHA256 {
		t.Fatal("historical fixture does not match the reviewed migration source")
	}
	var manifest struct {
		ProtocolURL string `json:"protocol_server_url"`
	}
	if err := json.Unmarshal(legacy, &manifest); err != nil || manifest.ProtocolURL == "" {
		t.Fatal("invalid historical fixture")
	}
	for name, mutate := range map[string]func([]byte) []byte{
		"origin only": func(raw []byte) []byte { return raw },
		"vault": func(raw []byte) []byte {
			return bytes.Replace(raw, []byte("0x49fA19f9bdECe7A48Ebc7749fD69aD40F577590F"), []byte("0x1111111111111111111111111111111111111111"), 1)
		},
		"deployment": func(raw []byte) []byte {
			return bytes.Replace(raw, []byte("fresh-20260930"), []byte("fresh-20260928"), 1)
		},
		"signer": func(raw []byte) []byte {
			return bytes.Replace(raw, []byte("0x25a4453190e930f9716eebcab165170706d31d1b3969d106a50d7ae374a23d61"), []byte("0x1"), 1)
		},
		"proof": func(raw []byte) []byte {
			return bytes.Replace(raw, []byte("c894b261a13f571d0df36be29734aabf2a8cd7162baddc5e08a50341aa076584"), []byte(strings.Repeat("a", 64)), 1)
		},
		"issuer": func(raw []byte) []byte {
			return bytes.Replace(raw, []byte("https://org-staging.openanonymity.ai"), []byte("https://untrusted.example"), 1)
		},
		"unknown origin": func(raw []byte) []byte {
			return bytes.ReplaceAll(raw, []byte(manifest.ProtocolURL), []byte("https://untrusted.example"))
		},
		"partial migration": func(raw []byte) []byte {
			return bytes.Replace(raw, []byte(manifest.ProtocolURL), []byte("https://zkapi-sepolia.openanonymity.ai"), 1)
		},
		"additional field": func(raw []byte) []byte {
			return bytes.Replace(raw, []byte("{\n"), []byte("{\n  \"unreviewed\": true,\n"), 1)
		},
	} {
		t.Run(name, func(t *testing.T) {
			dir := t.TempDir()
			path := filepath.Join(dir, "deployment-manifest.json")
			before := mutate(append([]byte(nil), legacy...))
			if err := os.WriteFile(path, before, 0600); err != nil {
				t.Fatal(err)
			}
			wallet := filepath.Join(dir, "wallet.json")
			const recovery = "synthetic private recovery sentinel"
			if err := os.WriteFile(wallet, []byte(recovery), 0600); err != nil {
				t.Fatal(err)
			}
			_, err := writeDeploymentManifest(dir, packaged)
			after, readErr := os.ReadFile(path)
			if readErr != nil {
				t.Fatal(readErr)
			}
			if name == "origin only" {
				if err != nil || !bytes.Equal(after, packaged) {
					t.Fatalf("origin migration failed: %v", err)
				}
				if _, err := writeDeploymentManifest(dir, packaged); err != nil {
					t.Fatal("migrated wallet did not restart", err)
				}
			} else if err == nil || !bytes.Equal(after, before) {
				t.Fatal("mismatched saved manifest accepted or changed")
			}
			info, _ := os.Stat(path)
			state, stateErr := os.ReadFile(wallet)
			if info.Mode().Perm() != 0600 || stateErr != nil || string(state) != recovery {
				t.Fatal("migration changed wallet recovery or manifest permissions")
			}
		})
	}
	for _, target := range [][]byte{
		bytes.Replace(packaged, []byte("fresh-20260930"), []byte("fresh-20260928"), 1),
		bytes.ReplaceAll(packaged, []byte("https://zkapi-sepolia.openanonymity.ai"), []byte("https://untrusted.example")),
		append(append([]byte(nil), packaged...), '\n'),
	} {
		if isSepoliaOriginMigration(legacy, target) {
			t.Fatal("actual prior manifest accepted an unreviewed replacement")
		}
	}
	_, mainnet, _ := pinnedDeployment("mainnet")
	oldMainnet := bytes.ReplaceAll(mainnet, []byte("https://zkapi-mainnet.openanonymity.ai"), []byte("https://retired.example"))
	if isSepoliaOriginMigration(oldMainnet, mainnet) {
		t.Fatal("Sepolia exception accepted mainnet")
	}
}

func TestCompanionPreservesRetiredMainnetProfile(t *testing.T) {
	binary, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	stateDir := t.TempDir()
	legacy := filepath.Join(stateDir, "mainnet", "zkapi-native-eth-mainnet-note-bound-v1-fresh-20260928")
	if err := os.MkdirAll(legacy, 0700); err != nil {
		t.Fatal(err)
	}
	wallet := filepath.Join(legacy, "wallet.json")
	const recovery = "synthetic legacy mainnet recovery"
	if err := os.WriteFile(wallet, []byte(recovery), 0600); err != nil {
		t.Fatal(err)
	}
	cmd, err := CompanionCommand(context.Background(), Config{Network: "mainnet", BridgeToken: testBridgeToken, HTTPClient: &http.Client{}}, CompanionConfig{Binary: binary, SetupDir: t.TempDir(), StateDir: stateDir, ProxyURL: "http://bridge:private-local-token@127.0.0.1:8791"})
	if err == nil || cmd != nil || !strings.Contains(err.Error(), "September 28") {
		t.Fatalf("retired profile accepted: %v", err)
	}
	state, _ := os.ReadFile(wallet)
	if string(state) != recovery {
		t.Fatal("retired wallet changed")
	}
	if _, err := os.Stat(filepath.Join(stateDir, "mainnet", "zkapi-native-eth-mainnet-note-bound-v1-fresh-20260930")); !os.IsNotExist(err) {
		t.Fatal("created another mainnet wallet beside retired state")
	}
}

func testProofSetup(t *testing.T) (string, proofSetupIdentity) {
	t.Helper()
	dir := t.TempDir()
	hashes := make(map[string]string)
	for _, name := range []string{"request.pk", "request.vk", "withdrawal.pk", "withdrawal.vk"} {
		contents := []byte("distinct test proving artifact " + name)
		if err := os.WriteFile(filepath.Join(dir, name), contents, 0600); err != nil {
			t.Fatal(err)
		}
		digest := sha256.Sum256(contents)
		hashes[name] = hex.EncodeToString(digest[:])
	}
	return dir, proofSetupIdentity{Circuit: "zkapi-v2-note-bound-v1", RequestProvingKeySHA256: hashes["request.pk"], RequestVerifyingKeySHA256: hashes["request.vk"], WithdrawalProvingKeySHA256: hashes["withdrawal.pk"], WithdrawalVerifyingKeySHA256: hashes["withdrawal.vk"]}
}

func TestProofSetupRequiresEveryPinnedArtifact(t *testing.T) {
	dir, proof := testProofSetup(t)
	if err := verifyProofSetup(dir, proof); err != nil {
		t.Fatalf("matching setup rejected: %v", err)
	}
	for _, name := range []string{"request.pk", "request.vk", "withdrawal.pk", "withdrawal.vk"} {
		for _, mutation := range []string{"modified", "missing", "directory"} {
			t.Run(name+"/"+mutation, func(t *testing.T) {
				dir, proof := testProofSetup(t)
				path := filepath.Join(dir, name)
				switch mutation {
				case "modified":
					if err := os.WriteFile(path, []byte("different proof setup"), 0600); err != nil {
						t.Fatal(err)
					}
				case "missing":
					if err := os.Remove(path); err != nil {
						t.Fatal(err)
					}
				case "directory":
					if err := os.Remove(path); err != nil {
						t.Fatal(err)
					}
					if err := os.Mkdir(path, 0700); err != nil {
						t.Fatal(err)
					}
				}
				if err := verifyProofSetup(dir, proof); err == nil || !strings.Contains(err.Error(), name) {
					t.Fatalf("%s %s accepted or wrong error: %v", mutation, name, err)
				}
			})
		}
	}
	for _, invalid := range []string{"", "abc", strings.Repeat("g", 64), strings.ToUpper(proof.RequestProvingKeySHA256)} {
		mutated := proof
		mutated.RequestProvingKeySHA256 = invalid
		if err := verifyProofSetup(dir, mutated); err == nil {
			t.Fatal("invalid artifact pin accepted")
		}
	}
}

func TestCompanionCommandChecksInstalledProofsBeforeCreatingWallet(t *testing.T) {
	binary, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	setupDir, _ := testProofSetup(t) // Valid files from a different setup must fail too.
	for _, dir := range []string{t.TempDir(), setupDir} {
		stateDir := filepath.Join(t.TempDir(), "must-not-exist")
		cmd, err := CompanionCommand(context.Background(), Config{Network: "sepolia", BridgeToken: testBridgeToken, HTTPClient: &http.Client{}}, CompanionConfig{Binary: binary, SetupDir: dir, StateDir: stateDir, ProxyURL: "http://bridge:private-local-token@127.0.0.1:8791"})
		if err == nil || cmd != nil || !strings.Contains(err.Error(), "request.pk") {
			t.Fatalf("companion did not reject wrong installed proofs: %v", err)
		}
		if _, err := os.Stat(stateDir); !os.IsNotExist(err) {
			t.Fatal("invalid setup created or opened private wallet state")
		}
	}
}

func TestMatchesDeploymentUsesExactNetworkIDAndVault(t *testing.T) {
	for _, network := range []string{"mainnet", "sepolia"} {
		deployment, _, err := pinnedDeployment(network)
		if err != nil {
			t.Fatal(err)
		}
		if !MatchesDeployment(network, deployment.ID, strings.ToLower(deployment.Contract)) {
			t.Fatal("matching deployment rejected")
		}
		if MatchesDeployment("unknown", deployment.ID, deployment.Contract) || MatchesDeployment(network, deployment.ID+"-other", deployment.Contract) || MatchesDeployment(network, deployment.ID, "0x1111111111111111111111111111111111111111") {
			t.Fatal("mismatched deployment accepted")
		}
	}
}
