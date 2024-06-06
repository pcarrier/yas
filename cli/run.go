package cli

import (
	"log"
	"os"
	"strconv"
)

func Run(env []string, args []string, in *os.File, out *os.File, errFile *os.File) {
	if len(args) < 2 {
		log.Fatalf("Usage: %s tool …\n", args[0])
	}
	url, frag, err := toolPath(args[1])
	if err != nil {
		log.Fatalf("Invalid tool reference: %v\n", err)
	}
	log.Printf("Would invoke %v with fragment %v\n", strconv.Quote(url), strconv.Quote(frag))
}
