require('string')
local io = require('io')
local os = require('os')

if #args < 1 then
  io.stderr:write("Arguments missing!\n")
  os.exit(1)
end
local f = assert(loadfile(args[1]))
local ok, err = pcall(f, args)
if not ok then
  io.stderr:write(err)
  os.exit(3)
end
