package main

import (
	"archive/zip"
	"crypto/sha256"
	"encoding/base64"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"testing"
)

func makeModule(t *testing.T, files map[string]string, duplicate bool) (string, string) {
	t.Helper()
	name := filepath.Join(t.TempDir(), "module.zip")
	f, err := os.Create(name)
	if err != nil {
		t.Fatal(err)
	}
	z := zip.NewWriter(f)
	names := make([]string, 0, len(files))
	for name := range files {
		names = append(names, name)
	}
	// Deliberately write reverse order; h1 must sort names independently.
	sort.Sort(sort.Reverse(sort.StringSlice(names)))
	for _, name := range names {
		w, err := z.Create(name)
		if err != nil {
			t.Fatal(err)
		}
		_, _ = w.Write([]byte(files[name]))
	}
	if duplicate {
		w, _ := z.Create(names[0])
		_, _ = w.Write([]byte(files[names[0]]))
	}
	if err := z.Close(); err != nil {
		t.Fatal(err)
	}
	_ = f.Close()
	sort.Strings(names)
	var lines strings.Builder
	for _, name := range names {
		fmt.Fprintf(&lines, "%x  %s\n", sha256.Sum256([]byte(files[name])), name)
	}
	sum := sha256.Sum256([]byte(lines.String()))
	return name, "h1:" + base64.StdEncoding.EncodeToString(sum[:])
}

func TestModuleHashSortsNamesAndChecksContents(t *testing.T) {
	prefix := "example.invalid/module@v1.0.0"
	file, sum := makeModule(t, map[string]string{prefix + "/目录/a.go": "first\n", prefix + "/go.mod": "module example.invalid/module\n"}, false)
	p, err := moduleHash(file, prefix, sum)
	if err != nil || p.Files != 2 || p.Version != 1 {
		t.Fatalf("%+v %v", p, err)
	}
	if _, err := moduleHash(file, prefix, "h1:"+base64.StdEncoding.EncodeToString(make([]byte, 32))); err == nil {
		t.Fatal("changed content hash accepted")
	}
}

func TestModuleHashRefusesDuplicateAndEscapingNames(t *testing.T) {
	prefix := "example.invalid/module@v1.0.0"
	for _, name := range []string{"../escape", prefix + "/../escape", prefix + "/a\n.go", prefix + "/a\\b"} {
		file, sum := makeModule(t, map[string]string{name: "fixture"}, false)
		if _, err := moduleHash(file, prefix, sum); err == nil {
			t.Fatalf("unsafe name accepted: %q", name)
		}
	}
	file, sum := makeModule(t, map[string]string{prefix + "/a.go": "fixture"}, true)
	if _, err := moduleHash(file, prefix, sum); err == nil {
		t.Fatal("duplicate accepted")
	}
}
