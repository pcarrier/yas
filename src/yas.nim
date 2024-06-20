import std/[strformat, strutils, appdirs, os, paths, httpclient, uri], lua, lmdb

const license = staticRead("../LICENSE")

proc help() =
  echo "Usage: yas [--help|--license|tool URL [args...]]"
  quit(1)

let parms = commandLineParams()
if parms.len == 0:
  help()
case parms[0]:
  of "--license":
    echo license
    quit(0)
  of "--help":
    help()
  else:
    let tool = parms[0]
    let base = getEnv("YAS_BASE", "https://oh.yas.tools/")
    let toolURL = combine(parseUri(base), parseUri(tool))
    let appDir = getAppDir()
    let pathInstalled = appDir in split(getEnv("PATH"), PathSep)
    let home = appdirs.getDataDir() / Path("yas")
    createDir(string(home))
    let e = newLMDBEnv(string(home))
    let l = newstate()
    l.openlibs
    if not pathInstalled:
      writeLine(stderr, &"Warning: {appDir} not in PATH. Install with {paramStr(0)} install")
    echo &"Hello, {cast[uint](l)}, {cast[uint](e)} for {toolURL} ({cast[string](home)})!"
    var client = newHttpClient()
    defer: close(client)
    echo client.getContent(toolURL)
