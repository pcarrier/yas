#!/usr/bin/env lua

local function str (x)
    return type(x) == "string" and ("%q"):format(x) or tostring(x)
end

local function dump (t, indent, already)
    indent, already = indent or 0, already or {}
    local keys = {}
    for k in pairs(t) do keys[#keys+1] = k end
    table.sort(keys)
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
