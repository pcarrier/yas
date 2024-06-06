package cli

import (
	"go.starlark.net/repl"
	"go.starlark.net/starlark"
	"go.starlark.net/syntax"
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

	thread := runtime.Thread(runtime.Tool)

	if runtime.Tool == "repl" {
		repl.REPLOptions(&syntax.FileOptions{}, thread, runtime.Globals())
	} else {
		src, err := runtime.Fetch(thread, runtime.Tool)
		if err != nil {
			log.Fatalf("Failed to load script %q: %v", runtime.Tool, err)
		}
		if _, err := starlark.ExecFileOptions(&syntax.FileOptions{}, thread, runtime.Tool, src, runtime.Globals()); err != nil {
			log.Fatalf("Script execution failed: %v", err)
		}
	}
}
