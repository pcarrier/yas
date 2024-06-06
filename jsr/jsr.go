package jsr

/*
#include "jsr/quickjs/quickjs.h"
#include "jsr/quickjs/quickjs-libc.h"
*/
import "C"

import "runtime"

type Value struct {
	ctx *Context
	ref C.JSValue
}

type Runtime struct {
	ref *C.JSRuntime
}

type Context struct {
	runtime    *Runtime
	ref        *C.JSContext
	globals    *Value
	proxy      *Value
	asyncProxy *Value
}

func (v Value) Free() {
	C.JS_FreeValue(v.ctx.ref, v.ref)
}

func NewRuntime() Runtime {
	runtime.LockOSThread()
	rt := C.JS_NewRuntime()
	C.js_std_init_handlers(rt)
	return Runtime{
		ref: rt,
	}
}

func (r Runtime) Close() {
	C.JS_FreeRuntime(r.ref)
}

func (r Runtime) NewContext() *Context {
	ref := C.JS_NewContext(r.ref)
	C.JS_AddIntrinsicBigFloat(ref)
	C.JS_AddIntrinsicBigDecimal(ref)
	C.JS_AddIntrinsicOperators(ref)
	C.JS_EnableBignumExt(ref, C.int(1))
	return &Context{ref: ref, runtime: &r}
}

func (ctx *Context) Close() {
	if ctx.proxy != nil {
		ctx.proxy.Free()
	}
	if ctx.asyncProxy != nil {
		ctx.asyncProxy.Free()
	}
	if ctx.globals != nil {
		ctx.globals.Free()
	}
	C.JS_FreeContext(ctx.ref)
}
