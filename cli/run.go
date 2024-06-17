package cli

import (
	"log"

	"yas.tools/rt"
)

func Run(envp []string, argv []string) {
	if len(argv) < 2 {
		log.Fatalf("Usage: %s <tool> [arg ...]", argv[0])
	}
	runtime, err := rt.NewRT(envp, argv)

	if err != nil {
		log.Fatalf("Failed to initialize runtime: %v", err)
	}

	src, err := runtime.Fetch(runtime.Base, runtime.Tool)
	if err != nil {
		log.Fatalf("Failed to load script %q: %v", runtime.Tool, err)
	}

	log.Printf("Would execute %q", src)
}
