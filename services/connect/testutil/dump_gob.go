package main

import (
	"bytes"
	"encoding/gob"
	"encoding/hex"
	"fmt"
	"os"
)

func init() {
	gob.Register([]interface{}{})
}

func main() {
	sessionValues := map[interface{}]interface{}{
		"id":       1,
		"username": "admin",
		"role":     1000,
		"status":   1,
		"group":    "default",
	}

	var buf bytes.Buffer
	enc := gob.NewEncoder(&buf)
	if err := enc.Encode(sessionValues); err != nil {
		fmt.Fprintf(os.Stderr, "gob encode error: %v\n", err)
		os.Exit(1)
	}

	data := buf.Bytes()
	fmt.Printf("Total length: %d bytes\n", len(data))
	fmt.Printf("Hex: %s\n\n", hex.EncodeToString(data))

	// Print byte-by-byte with annotations
	for i, b := range data {
		if b >= 0x20 && b <= 0x7e {
			fmt.Printf("[%3d] 0x%02x '%c'\n", i, b, b)
		} else {
			fmt.Printf("[%3d] 0x%02x\n", i, b)
		}
	}

	// Also decode it back to verify
	fmt.Println("\n--- Decode verification ---")
	var decoded map[interface{}]interface{}
	dec := gob.NewDecoder(bytes.NewReader(data))
	if err := dec.Decode(&decoded); err != nil {
		fmt.Fprintf(os.Stderr, "decode error: %v\n", err)
		os.Exit(1)
	}
	for k, v := range decoded {
		fmt.Printf("  %v (%T) => %v (%T)\n", k, k, v, v)
	}
}
