require('string')
local io = require('io')
local tsort = require('table').sort

local function help (name)
    io.stderr:write("Usage: " .. name .. " [--help | --license | URL [args...]]\n")
end

local function str (x)
    return type(x) == "string" and ("%q"):format(x) or tostring(x)
end

do -- turn A=B entries into a nice map
    local args = yas.args
    local script = args[0]

    if script == nil or script == "--help" then
        help(args[-1])
        return
    elseif script == "--license" then
        io.stdout:write(yas.license)
        return
    end

    local sillyEnv = yas.env
    local env = {}
    for _, e in next, sillyEnv do
        local k, v = e:match("([^=]*)=(.*)")
        env[k] = v
    end
    yas.env = env
end

local function dump (t, indent, already)
    indent, already = indent or 0, already or {}
    local keys = {}
    for k in pairs(t) do keys[#keys+1] = k end
    tsort(keys)
    for _, k in pairs(keys) do
        local v = t[k]
        if type(v) == "table" then
            if already[v] then
                print(("%s[%s] = %s"):format((" "):rep(indent), str(k), str(v)))
            else
                already[v] = true
                print(("%s[%s] = {"):format((" "):rep(indent), str(k)))
                dump(v, indent + 2, already)
                print(("%s}"):format((" "):rep(indent)))
            end
        else
            print(("%s[%s] = %s"):format((" "):rep(indent), str(k), str(v)))
        end
    end
end

dump(_G)
