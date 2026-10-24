// Build-time verification of the Go dirhash.Hash1 zip-content format.
// Source archives are inspected as data; no entry is extracted or executed.
package main

import (
	"archive/zip"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"path"
	"sort"
	"strings"
	"unicode/utf8"
)

type proof struct {
	Version int    `json:"version"`
	Sum     string `json:"sum"`
	Files   int    `json:"files"`
	Bytes   int64  `json:"bytes"`
}

func moduleHash(file, prefix, expected string) (proof, error) {
	var result proof
	if prefix == "" || strings.HasPrefix(prefix, "/") || path.Clean(prefix) != prefix || strings.ContainsAny(prefix, "\\\r\n\x00") {
		return result, errors.New("invalid module prefix")
	}
	want, err := base64.StdEncoding.DecodeString(strings.TrimPrefix(expected, "h1:"))
	if err != nil || len(want) != sha256.Size || expected != "h1:"+base64.StdEncoding.EncodeToString(want) {
		return result, errors.New("invalid expected h1")
	}
	z, err := zip.OpenReader(file)
	if err != nil {
		return result, err
	}
	defer z.Close()
	if len(z.File) == 0 || len(z.File) > 20000 {
		return result, errors.New("module entry budget exceeded")
	}
	entries := append([]*zip.File(nil), z.File...)
	sort.Slice(entries, func(i, j int) bool { return entries[i].Name < entries[j].Name })
	listing := sha256.New()
	previous := ""
	for _, entry := range entries {
		name := entry.Name
		clean := strings.TrimSuffix(name, "/")
		if !utf8.ValidString(name) || !strings.HasPrefix(name, prefix+"/") || path.Clean(clean) != clean || name == previous || strings.ContainsAny(name, "\\\r\n\x00") {
			return result, errors.New("invalid, duplicate or escaping module entry")
		}
		for _, r := range name {
			if r < 32 || r == 127 {
				return result, errors.New("control character in module entry")
			}
		}
		mode := entry.Mode()
		if (!mode.IsRegular() && !mode.IsDir()) || mode&(os.ModeSetuid|os.ModeSetgid|os.ModeSticky) != 0 || entry.UncompressedSize64 > 64<<20 {
			return result, errors.New("unsupported module entry type or size")
		}
		r, err := entry.Open()
		if err != nil {
			return result, err
		}
		content := sha256.New()
		n, copyErr := io.Copy(content, io.LimitReader(r, (64<<20)+1))
		closeErr := r.Close()
		if copyErr != nil || closeErr != nil || uint64(n) != entry.UncompressedSize64 || n > 64<<20 {
			return result, errors.New("module content or CRC could not be verified")
		}
		result.Bytes += n
		if result.Bytes > 512<<20 {
			return result, errors.New("module expanded-size budget exceeded")
		}
		// Hash1 hashes sorted file-content hashes and names, not zip encodings.
		_, _ = io.WriteString(listing, hex.EncodeToString(content.Sum(nil))+"  "+name+"\n")
		previous = name
	}
	result.Version, result.Files = 1, len(entries)
	result.Sum = "h1:" + base64.StdEncoding.EncodeToString(listing.Sum(nil))
	if result.Sum != expected {
		return result, errors.New("Go module h1 mismatch")
	}
	return result, nil
}

func main() {
	file := flag.String("zip", "", "module zip")
	prefix := flag.String("module", "", "module@version prefix")
	expected := flag.String("sum", "", "expected h1")
	flag.Parse()
	if flag.NArg() != 0 {
		fmt.Fprintln(os.Stderr, "unexpected arguments")
		os.Exit(1)
	}
	result, err := moduleHash(*file, *prefix, *expected)
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	_ = json.NewEncoder(os.Stdout).Encode(result)
}
