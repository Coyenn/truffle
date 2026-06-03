--!strict
-- UTF-8 helpers (Luau #string and string.sub index bytes, not Unicode scalars).

local Utf8Util = {}

function Utf8Util.nextChar(str: string, byteIndex: number): (string?, number)
	if byteIndex > #str then
		return nil, byteIndex
	end

	local sub = string.sub(str, byteIndex)
	for start, stop in utf8.graphemes(sub) do
		local charStart = byteIndex + start - 1
		local charStop = byteIndex + stop - 1
		return string.sub(str, charStart, charStop), charStop + 1
	end

	return nil, byteIndex + 1
end

function Utf8Util.graphemeCount(str: string): number
	return utf8.len(str) or 0
end

return Utf8Util
