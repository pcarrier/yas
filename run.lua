local utils = require("utils")
local build = require("builds")
local p = require("platforms")
local l = require("local")

local o = p.operating_systems[l.os]
if not o then error("Invalid OS " .. l.os) end
local exe = build(o, l.arch)
utils.exec(exe, ...)
