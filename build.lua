local utils = require("utils")
local build = require("builds")
local p = require("platforms")

if #arg > 0 then
    for _, os_arch in ipairs(arg) do
        local os, arch = os_arch:match("([^-]+)-([^-]+)")
        if (not os) or (not arch) then
            error("Invalid argument: expected $os-$arch instead of " .. os_arch)
        end
        local o = p.operating_systems[os]
        if not o then error("Invalid OS " .. os .. ": pass one of " .. table.cat(utils.keys(operating_systems), ", ")) end
        build(o, arch)
    end
else
    for _, os in next, p.operating_systems do
        for _, arch in next, p.architectures do
            build(os, arch)
        end
    end
end
