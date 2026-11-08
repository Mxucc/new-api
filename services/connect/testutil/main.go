package main

import (
	"bytes"
	"encoding/gob"
	"encoding/hex"
	"fmt"
	"os"

	"github.com/gorilla/securecookie"
)

func init() {
	gob.Register([]interface{}{})
}

func main() {
	secret := "test-session-secret-123"
	if len(os.Args) > 1 {
		secret = os.Args[1]
	}

	sc := securecookie.New([]byte(secret), nil)
	sc.MaxAge(86400 * 30)

	sessionValues := map[interface{}]interface{}{
		"id":       1,
		"username": "admin",
		"role":     1000,
		"status":   1,
		"group":    "default",
	}

	// Encode the full cookie
	encoded, err := sc.Encode("session", sessionValues)
	if err != nil {
		fmt.Fprintf(os.Stderr, "encode error: %v\n", err)
		os.Exit(1)
	}
	fmt.Printf("COOKIE_VALUE=%s\n", encoded)

	// Also dump the raw gob bytes for testing the gob decoder
	var buf bytes.Buffer
	enc := gob.NewEncoder(&buf)
	if err := enc.Encode(sessionValues); err != nil {
		fmt.Fprintf(os.Stderr, "gob encode error: %v\n", err)
		os.Exit(1)
	}
	fmt.Printf("GOB_HEX=%s\n", hex.EncodeToString(buf.Bytes()))
	fmt.Printf("SECRET=%s\n", secret)
}
