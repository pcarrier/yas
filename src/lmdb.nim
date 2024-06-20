type
  cursor* {.incompleteStruct.} = object
  LMDBCursor* = ptr cursor

when defined(windows):
  type ModeT* = cint
else:
  import posix
  type ModeT* = posix.Mode

type
  Env* = object
  LMDBEnv* = ptr Env
  Txn* = object
  LMDBTxn* = ptr Txn
  Dbi* = cuint
  Val* {.bycopy.} = object
    mvSize*: uint
    mvData*: pointer
  CmpFunc* = proc (a: ptr Val; b: ptr Val): cint {.cdecl.}
  RelFunc* = proc (item: ptr Val; oldptr: pointer; newptr: pointer; relctx: pointer) {.cdecl.}

proc strerror*(err: cint): cstring {.cdecl, importc: "mdb_strerror".}
proc envCreate*(env: ptr LMDBEnv): cint {.cdecl, importc: "mdb_env_create".}
proc envOpen*(env: LMDBEnv; path: cstring; flags: cuint; mode: ModeT): cint {.cdecl, importc: "mdb_env_open".}

template check(err: cint) =
  if err != 0:
    let s = $strerror(err)
    raise newException(Exception, s)

proc newLMDBEnv*(path: string, openflags=0): LMDBEnv =
  var env: LMDBEnv
  check envCreate(addr(env))
  check envOpen(env, path.cstring, openflags.cuint, 0o0664)
  return env
