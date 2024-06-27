local exec = require("utils").exec
local unpack = require("table").unpack
local build = require("builds")
local p = require("platforms")
local b = require("buildalib")

require("bootstrapReady")

local o = p.operating_systems[b.os]
if not o then error("Invalid OS " .. l.os) end
local exe = build(o, b.arch)
exec(exe, unpack(args))
