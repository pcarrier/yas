local M, name_ = {}, {}

local replacements = { ["&"] = "&amp;", ["<"] = "&lt;", [">"] = "&gt;", ['"'] = "&quot;", }

local function escape(s)
    return s:gsub("[&<>]", replacements)
end

local function qescape(s)
    return s:gsub('[&<>"]', replacements)
end

local mt = {
    __tostring = function(t)
        local attrs, children = {}, {}
        for k, v in next, t do
            if type(k) == "number" then
                children[k] = v
            else
                attrs[k] = v
            end
        end
        local parts = { "<", t[name_] }
        for k, v in next, attrs do
            if k ~= name_ then
                parts[#parts + 1] = " " .. tostring(k) .. '="' .. qescape(v) .. '"'
            end
        end
        if #children == 0 then
            parts[#parts + 1] = "/>"
        else
            parts[#parts + 1] = ">"
            for _, v in ipairs(children) do
                parts[#parts + 1] = type(v) == "string" and escape(v) or tostring(v)
            end
            parts[#parts + 1] = "</" .. t[name_] .. ">"
        end
        return table.concat(parts)
    end
}

setmetatable(M, {
    __index = function(_, name)
        return function(arg)
            if type(arg) ~= "table" then
                arg = { arg }
            end
            setmetatable(arg, mt)
            arg[name_] = name
            return arg
        end
    end
})

return M
