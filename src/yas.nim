import std/[strformat, strutils, appdirs, os, paths, httpclient, uri]

let parms = commandLineParams()
let tool = parms[0]
let toolURL = combine(parseUri(base), parseUri(tool))
let e = newLMDBEnv(string(home))
let l = newstate()
l.openlibs
if not pathInstalled:
  writeLine(stderr, &"Warning: {appDir} not in PATH. Install with {paramStr(0)} install")
echo &"Hello, {cast[uint](l)}, {cast[uint](e)} for {toolURL} ({cast[string](home)})!"
var client = newHttpClient()
defer: close(client)
echo client.getContent(toolURL)
