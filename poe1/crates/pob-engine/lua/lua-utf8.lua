-- Pure-Lua stand-in for the `lua-utf8` native module.
--
-- PoB ships lua-utf8 as a compiled library (runtime/lua-utf8.dll) and so only
-- has it on Windows. We need the calc modules on macOS/Linux/iPadOS, so this
-- supplies the handful of entry points PoB actually calls.
--
-- Scope check before writing this: `utf8.` appears 12 times in PoB's source.
-- Six are in Classes/EditControl.lua, a text-entry widget we never load (we
-- draw our own UI in egui). The rest are Modules/Common.lua's formatNumber,
-- which inserts thousands separators for *display*. No calculation path
-- depends on this module, so the shim cannot move a number.
--
-- Where a real UTF-8 implementation would differ from these byte-oriented
-- versions, it only matters for multi-byte separators; PoB's separators are
-- ASCII ("," and "."), so the results agree.

local m = {}

m.find = string.find
m.gsub = string.gsub
m.sub = string.sub
m.len = string.len
m.upper = string.upper
m.lower = string.lower

--- string.match, used by EditControl for word-boundary caret motion.
m.match = string.match

--- Reverse a UTF-8 string by codepoint rather than by byte.
function m.reverse(s)
	local out, i, n = {}, 1, #s
	while i <= n do
		local c = s:byte(i)
		-- Width from the leading byte; continuation bytes are 0x80..0xBF.
		local w = c < 0x80 and 1 or c < 0xE0 and 2 or c < 0xF0 and 3 or 4
		table.insert(out, 1, s:sub(i, i + w - 1))
		i = i + w
	end
	return table.concat(out)
end

--- Byte offset of the codepoint `n` steps from `i` (n may be negative).
--- Returns nil when the walk runs off either end, matching lua-utf8.
function m.next(s, i, n)
	i = i or 1
	n = n or 1
	local len = #s
	local function is_cont(pos)
		local b = s:byte(pos)
		return b and b >= 0x80 and b < 0xC0
	end
	while n > 0 do
		if i > len then return nil end
		i = i + 1
		while i <= len and is_cont(i) do i = i + 1 end
		n = n - 1
	end
	while n < 0 do
		if i <= 1 then return nil end
		i = i - 1
		while i > 1 and is_cont(i) do i = i - 1 end
		n = n + 1
	end
	return i
end

return m
