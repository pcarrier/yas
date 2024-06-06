package main

import "github.com/buke/quickjs-go"

func main() {
	rt := quickjs.NewRuntime()
	defer rt.Close()
	ctx := rt.NewContext()
	defer ctx.Close()
	ret, err := ctx.Eval("1 + 2")
	if err != nil {
		panic(err)
	}
	println(ret.Int32())
}
