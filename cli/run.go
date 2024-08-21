package cli

import (
	"log"
	"net/url"

	"go.starlark.net/repl"
	"go.starlark.net/starlark"
	"go.starlark.net/syntax"
	"yas.tools/rt"
)

func Run(envp []string, argv []string) {
	runtime, err := rt.NewRT(envp, argv)

	if err != nil {
		log.Fatalf("Failed to initialize runtime: %v", err)
	}

	thread := runtime.Thread(runtime.Tool)

	if runtime.Tool == "repl" {
		repl.REPLOptions(&syntax.FileOptions{}, thread, runtime.Globals())
	} else {
		toolUrl, err := url.Parse(runtime.Tool)
		if err != nil {
			log.Fatalf("Failed to parse tool URL %q: %v", runtime.Tool, err)
		}
		src, err := runtime.Fetch(toolUrl)
		if err != nil {
			log.Fatalf("Could not load script %q: %v", runtime.Tool, err)
		}
		if _, err := starlark.ExecFileOptions(&syntax.FileOptions{}, thread, runtime.Tool, src, runtime.Globals()); err != nil {
			log.Fatalf("Script execution failed: %v", err)
		}
	}
}
