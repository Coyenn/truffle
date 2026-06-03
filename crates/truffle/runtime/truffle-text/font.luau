--!strict
-- Platform-agnostic font metrics loaded from Truffle font metadata.

local Utf8Util = require(script.Parent.utf8_util)

local Font = {}
Font.__index = Font

export type FontMetaV2 = {
	lineHeight: number,
	baseline: number,
	px: number,
	charset: string,
	pages: { { w: number, h: number } },
	advances: { number },
	offsetX: { number },
	offsetY: { number },
	rects: { number },
	kerning: {
		leftClass: { number },
		rightClass: { number },
		leftCount: number,
		rightCount: number,
		matrix: { number },
	},
}

export type FontHandle = typeof(setmetatable({} :: {
	meta: FontMetaV2,
	indexByChar: { [string]: number },
}, Font))

function Font.fromMeta(meta: FontMetaV2): FontHandle
	local indexByChar: { [string]: number } = {}
	local charIndex = 0
	local byteIndex = 1
	while byteIndex <= #meta.charset do
		local ch, nextByte = Utf8Util.nextChar(meta.charset, byteIndex)
		if ch == nil then
			break
		end
		indexByChar[ch] = charIndex
		charIndex += 1
		byteIndex = nextByte
	end
	return setmetatable({
		meta = meta,
		indexByChar = indexByChar,
	}, Font)
end

function Font:getLineHeight(): number
	return self.meta.lineHeight
end

function Font:getPx(): number
	return self.meta.px
end

function Font:getCharset(): string
	return self.meta.charset
end

function Font:charIndex(character: string): number?
	return self.indexByChar[character]
end

function Font:getAdvance(character: string): number
	local idx = self:charIndex(character)
	if idx == nil then
		return 0
	end
	return self.meta.advances[idx + 1] or 0
end

function Font:getKerning(left: string, right: string): number
	local li = self:charIndex(left)
	local ri = self:charIndex(right)
	if li == nil or ri == nil then
		return 0
	end
	local kerning = self.meta.kerning
	local lClass = kerning.leftClass[li + 1]
	local rClass = kerning.rightClass[ri + 1]
	if lClass == nil or rClass == nil then
		return 0
	end
	local rc = kerning.rightCount
	local mi = lClass * rc + rClass + 1
	return kerning.matrix[mi] or 0
end

function Font:getGlyphRect(character: string): (number?, number?, number?, number?, number?)
	local idx = self:charIndex(character)
	if idx == nil then
		return nil
	end
	local base = idx * 5 + 1
	local rects = self.meta.rects
	local page = rects[base]
	local x = rects[base + 1]
	local y = rects[base + 2]
	local w = rects[base + 3]
	local h = rects[base + 4]
	if w == nil or h == nil or w <= 0 or h <= 0 then
		return nil
	end
	return page, x, y, w, h
end

function Font:getOffset(character: string): (number, number)
	local idx = self:charIndex(character)
	if idx == nil then
		return 0, 0
	end
	return self.meta.offsetX[idx + 1] or 0, self.meta.offsetY[idx + 1] or 0
end

return Font
