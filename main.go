package main

import (
	"github.com/yas-tools/yas/cli"
	"os"
)

func main() {
	env := os.Environ()
	args := os.Args
	in := os.Stdin
	out := os.Stdout
	err := os.Stderr
	cli.Run(env, args, in, out, err)
}
