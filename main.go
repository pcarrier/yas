package main

import (
	"os"
	"yas.tools/cli"
)

func main() {
	cli.Run(os.Environ(), os.Args)
}
