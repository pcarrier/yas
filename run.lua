local utils = require("utils")
local build = require("builds")
local p = require("platforms")
local b = require("buildalib")

local o = p.operating_systems[b.os]
if not o then error("Invalid OS " .. l.os) end
local exe = build(o, b.arch)
utils.exec(exe, ...)
