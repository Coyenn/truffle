--!strict
-- Platform-agnostic text layout (no rendering).

local FontModule = require(script.Parent.font)
local RichText = require(script.Parent.rich_text)
local Utf8Util = require(script.Parent.utf8_util)

export type FontHandle = FontModule.FontHandle

export type LayoutOptions = {
	textSize: number?,
	lineHeight: number?,
	letterSpacing: number?,
	lineSpacing: number?,
	maxWidth: number?,
	maxHeight: number?,
	truncate: boolean?,
	alignX: "left" | "center" | "right"?,
	alignY: "top" | "center" | "bottom"?,
}

export type LayoutGlyph = {
	character: string,
	index: number,
	x: number,
	y: number,
	width: number,
	height: number,
	advance: number,
	atlasPage: number,
	srcX: number,
	srcY: number,
	srcW: number,
	srcH: number,
	offsetX: number,
	offsetY: number,
	attrs: { [string]: any },
}

export type LayoutResult = {
	glyphs: { LayoutGlyph },
	width: number,
	height: number,
	lineCount: number,
	truncated: boolean,
	scale: number,
}

local ELLIPSIS = "..."
local ELLIPSIS_DOT_COUNT = 3

local Layout = {}

local function resolveScale(font: FontHandle, options: LayoutOptions): (number, number)
	local design = font:getLineHeight()
	local textSize = options.textSize or design
	local scale = textSize / design
	local lineAdvance = options.lineHeight or textSize
	return scale, lineAdvance
end

local function isWhitespace(character: string): boolean
	return character == " " or character == "\t"
end

type CharacterData = {
	character: string,
	index: number,
	kerningBefore: number,
	advance: number,
	hasGlyph: boolean,
	offsetX: number,
	offsetY: number,
	srcPage: number,
	srcX: number,
	srcY: number,
	srcW: number,
	srcH: number,
	attrs: { [string]: any },
}

local function buildCharacterData(
	font: FontHandle,
	character: string,
	index: number,
	previousCharacter: string?,
	attrs: { [string]: any },
	scale: number
): CharacterData
	local advance = font:getAdvance(character) * scale
	local kerningBefore = 0
	if previousCharacter ~= nil then
		kerningBefore = font:getKerning(previousCharacter, character) * scale
		if character == "," and kerningBefore < 0 then
			kerningBefore = 0
		end
	end

	local page, sx, sy, sw, sh = font:getGlyphRect(character)
	local ox, oy = font:getOffset(character)
	local hasGlyph = page ~= nil and sw ~= nil and sh ~= nil and sw > 0 and sh > 0

	return {
		character = character,
		index = index,
		kerningBefore = kerningBefore,
		advance = advance,
		hasGlyph = hasGlyph,
		offsetX = ox * scale,
		offsetY = oy * scale,
		srcPage = page or 0,
		srcX = sx or 0,
		srcY = sy or 0,
		srcW = sw or 0,
		srcH = sh or 0,
		attrs = attrs,
	}
end

type WordData = {
	characters: { CharacterData },
	width: number,
}

type LineData = {
	words: { WordData },
	width: number,
}

local function buildWordFromChars(
	font: FontHandle,
	chars: { { character: string, index: number, attrs: { [string]: any } } },
	letterSpacing: number,
	scale: number
): WordData
	local characters: { CharacterData } = {}
	local width = 0
	local previous: string? = nil
	for _, item in chars do
		local data = buildCharacterData(font, item.character, item.index, previous, item.attrs, scale)
		table.insert(characters, data)
		width += data.kerningBefore + data.advance + letterSpacing
		previous = item.character
	end
	return { characters = characters, width = width }
end

local function buildWordFromString(
	font: FontHandle,
	word: string,
	letterSpacing: number,
	scale: number
): WordData
	local chars = {}
	local byteIndex = 1
	while byteIndex <= #word do
		local ch, nextByte = Utf8Util.nextChar(word, byteIndex)
		if ch == nil then
			break
		end
		local idx = font:charIndex(ch)
		table.insert(chars, {
			character = ch,
			index = idx or 0,
			attrs = {},
		})
		byteIndex = nextByte
	end
	return buildWordFromChars(font, chars, letterSpacing, scale)
end

local function splitWordAtWidth(
	font: FontHandle,
	word: WordData,
	maxWidth: number,
	letterSpacing: number,
	scale: number
): (WordData, WordData?)
	if word.width <= maxWidth or #word.characters <= 1 then
		return word, nil
	end

	local width = 0
	local splitIndex = 0
	for i, charData in word.characters do
		local charWidth = charData.kerningBefore + charData.advance + letterSpacing
		if width > 0 and width + charWidth > maxWidth then
			break
		end
		width += charWidth
		splitIndex = i
	end

	if splitIndex <= 0 then
		splitIndex = 1
	end
	if splitIndex >= #word.characters then
		return word, nil
	end

	local firstChars = {}
	local firstWidth = 0
	for i = 1, splitIndex do
		local charData = word.characters[i]
		table.insert(firstChars, charData)
		firstWidth += charData.kerningBefore + charData.advance + letterSpacing
	end

	local secondChars = {}
	local secondWidth = 0
	local previous: string? = nil
	for i = splitIndex + 1, #word.characters do
		local old = word.characters[i]
		local data = buildCharacterData(font, old.character, old.index, previous, old.attrs, scale)
		table.insert(secondChars, data)
		secondWidth += data.kerningBefore + data.advance + letterSpacing
		previous = old.character
	end

	return { characters = firstChars, width = firstWidth }, { characters = secondChars, width = secondWidth }
end

local function layoutLines(
	font: FontHandle,
	processLine: (lineText: string) -> (),
	textLines: { string }
)
	for _, lineText in textLines do
		processLine(lineText)
	end
end

local function finalizeLayout(
	font: FontHandle,
	lines: { LineData },
	options: LayoutOptions,
	scale: number,
	lineAdvance: number,
	letterSpacing: number,
	truncated: boolean
): LayoutResult
	local glyphs: { LayoutGlyph } = {}
	local maxLineWidth = 0
	for _, line in lines do
		if line.width > maxLineWidth then
			maxLineWidth = line.width
		end
	end

	local totalHeight = #lines * lineAdvance + math.max(0, #lines - 1) * (options.lineSpacing or 0)
	local startY = 0
	local alignY = options.alignY or "top"
	local maxHeight = options.maxHeight
	if alignY == "center" then
		startY = math.floor(((maxHeight or totalHeight) - totalHeight) / 2 + 0.5)
	elseif alignY == "bottom" then
		startY = (maxHeight or totalHeight) - totalHeight
	end

	local lineSpacing = options.lineSpacing or 0
	for lineIndex, line in lines do
		local startX = 0
		local alignX = options.alignX or "left"
		if alignX == "center" then
			startX = math.floor(((options.maxWidth or maxLineWidth) - line.width) / 2 + 0.5)
		elseif alignX == "right" then
			startX = (options.maxWidth or maxLineWidth) - line.width
		end

		local currentX = startX
		local currentY = startY + (lineIndex - 1) * (lineAdvance + lineSpacing)
		local spaceWidth = (font:getAdvance(" ") + font:getKerning(" ", " ")) * scale + letterSpacing

		for _, word in line.words do
			for _, charData in word.characters do
				currentX += charData.kerningBefore
				if charData.hasGlyph then
					table.insert(glyphs, {
						character = charData.character,
						index = charData.index,
						x = currentX + charData.offsetX,
						y = currentY + charData.offsetY,
						width = charData.srcW * scale,
						height = charData.srcH * scale,
						advance = charData.advance,
						atlasPage = charData.srcPage,
						srcX = charData.srcX,
						srcY = charData.srcY,
						srcW = charData.srcW,
						srcH = charData.srcH,
						offsetX = charData.offsetX,
						offsetY = charData.offsetY,
						attrs = charData.attrs,
					})
				end
				currentX += charData.advance + letterSpacing
			end
			currentX += spaceWidth
		end
	end

	return {
		glyphs = glyphs,
		width = maxLineWidth,
		height = totalHeight,
		lineCount = #lines,
		truncated = truncated,
		scale = scale,
	}
end

function Layout.measureText(font: FontHandle, text: string, options: LayoutOptions?): { width: number, height: number }
	local result = Layout.layoutText(font, text, options or {})
	return { width = result.width, height = result.height }
end

function Layout.layoutText(font: FontHandle, text: string, options: LayoutOptions?): LayoutResult
	options = options or {}
	local scale, lineAdvance = resolveScale(font, options)
	local letterSpacing = options.letterSpacing or 0
	local maxWidth = options.maxWidth
	local maxHeight = options.maxHeight
	local truncate = options.truncate or false

	local spaceWidth = (font:getAdvance(" ") + font:getKerning(" ", " ")) * scale + letterSpacing
	local dotWidth = buildCharacterData(font, ".", font:charIndex(".") or 0, nil, {}, scale).advance + letterSpacing
	local ellipsisWidth = dotWidth * ELLIPSIS_DOT_COUNT

	local lines: { LineData } = {}
	local currentLineWords: { WordData } = {}
	local currentLineWidth = 0
	local truncated = false

	local function pushLine()
		table.insert(lines, {
			words = currentLineWords,
			width = math.max(0, currentLineWidth - spaceWidth),
		})
		currentLineWords = {}
		currentLineWidth = 0
	end

	local function performTruncation()
		local lastLine = lines[#lines]
		if lastLine == nil then
			return
		end
		if #lastLine.words == 0 then
			local dots = buildWordFromString(font, ELLIPSIS, letterSpacing, scale)
			lastLine.words = { dots }
			lastLine.width = dots.width
			return
		end

		local limit = (maxWidth or 0) - ellipsisWidth
		if limit < 0 then
			limit = 0
		end
		local newWords: { WordData } = {}
		local currentWidth = 0
		for _, word in lastLine.words do
			if currentWidth + word.width > limit then
				break
			end
			table.insert(newWords, word)
			currentWidth += word.width + spaceWidth
		end
		local dots = buildWordFromString(font, ELLIPSIS, letterSpacing, scale)
		table.insert(newWords, dots)
		lastLine.words = newWords
		lastLine.width = currentWidth + dots.width
	end

	local function processLine(lineText: string)
		if truncated then
			return
		end
		if lineText == "" then
			if #currentLineWords > 0 then
				pushLine()
			end
			pushLine()
			return
		end

		for wordText in string.gmatch(lineText, "[^ ]+") do
			local wordData = buildWordFromString(font, wordText, letterSpacing, scale)
			if maxWidth ~= nil and currentLineWidth + wordData.width > maxWidth then
				if currentLineWidth > 0 then
					pushLine()
					local currentHeight = (#lines + 1) * lineAdvance
					if truncate and maxHeight ~= nil and currentHeight > maxHeight then
						truncated = true
						performTruncation()
						return
					end
				end
				while wordData.width > maxWidth do
					local firstPart, secondPart = splitWordAtWidth(font, wordData, maxWidth, letterSpacing, scale)
					table.insert(currentLineWords, firstPart)
					currentLineWidth += firstPart.width
					pushLine()
					local currentHeight = (#lines + 1) * lineAdvance
					if truncate and maxHeight ~= nil and currentHeight > maxHeight then
						truncated = true
						performTruncation()
						return
					end
					if secondPart == nil then
						break
					end
					wordData = secondPart
				end
			end
			table.insert(currentLineWords, wordData)
			currentLineWidth += wordData.width + spaceWidth
		end
		pushLine()
		local currentHeight = #lines * lineAdvance
		if truncate and maxHeight ~= nil and currentHeight > maxHeight then
			while #lines * lineAdvance > maxHeight and #lines > 0 do
				table.remove(lines)
			end
			truncated = true
			performTruncation()
		end
	end

	local textLines = string.split(text, "\n")
	for _, lineText in textLines do
		processLine(lineText)
		if truncated then
			break
		end
	end

	return finalizeLayout(font, lines, options, scale, lineAdvance, letterSpacing, truncated)
end

function Layout.layoutRichText(
	font: FontHandle,
	spans: { RichText.RichTextSpan },
	options: LayoutOptions?
): LayoutResult
	options = options or {}
	local scale, lineAdvance = resolveScale(font, options)
	local letterSpacing = options.letterSpacing or 0
	local maxWidth = options.maxWidth
	local maxHeight = options.maxHeight
	local truncate = options.truncate or false

	local spaceWidth = (font:getAdvance(" ") + font:getKerning(" ", " ")) * scale + letterSpacing
	local dotWidth = buildCharacterData(font, ".", font:charIndex(".") or 0, nil, {}, scale).advance + letterSpacing
	local ellipsisWidth = dotWidth * ELLIPSIS_DOT_COUNT

	local lines: { LineData } = {}
	local currentLineWords: { WordData } = {}
	local currentLineWidth = 0
	local truncated = false
	local currentWord: { { character: string, index: number, attrs: { [string]: any } } } = {}

	local function pushLine()
		table.insert(lines, {
			words = currentLineWords,
			width = math.max(0, currentLineWidth - spaceWidth),
		})
		currentLineWords = {}
		currentLineWidth = 0
	end

	local function performTruncation()
		local lastLine = lines[#lines]
		if lastLine == nil then
			return
		end
		if #lastLine.words == 0 then
			local dots = buildWordFromString(font, ELLIPSIS, letterSpacing, scale)
			lastLine.words = { dots }
			lastLine.width = dots.width
			return
		end
		local limit = (maxWidth or 0) - ellipsisWidth
		if limit < 0 then
			limit = 0
		end
		local newWords: { WordData } = {}
		local currentWidth = 0
		for _, word in lastLine.words do
			if currentWidth + word.width > limit then
				break
			end
			table.insert(newWords, word)
			currentWidth += word.width + spaceWidth
		end
		local dots = buildWordFromString(font, ELLIPSIS, letterSpacing, scale)
		table.insert(newWords, dots)
		lastLine.words = newWords
		lastLine.width = currentWidth + dots.width
	end

	local function flushWord()
		if #currentWord == 0 then
			return
		end
		local wordData = buildWordFromChars(font, currentWord, letterSpacing, scale)
		currentWord = {}
		if maxWidth ~= nil and currentLineWidth + wordData.width > maxWidth then
			if currentLineWidth > 0 then
				pushLine()
				local currentHeight = (#lines + 1) * lineAdvance
				if truncate and maxHeight ~= nil and currentHeight > maxHeight then
					truncated = true
					performTruncation()
					return
				end
			end
			while wordData.width > maxWidth do
				local firstPart, secondPart = splitWordAtWidth(font, wordData, maxWidth, letterSpacing, scale)
				table.insert(currentLineWords, firstPart)
				currentLineWidth += firstPart.width
				pushLine()
				local currentHeight = (#lines + 1) * lineAdvance
				if truncate and maxHeight ~= nil and currentHeight > maxHeight then
					truncated = true
					performTruncation()
					return
				end
				if secondPart == nil then
					break
				end
				wordData = secondPart
			end
		end
		table.insert(currentLineWords, wordData)
		currentLineWidth += wordData.width + spaceWidth
	end

	for _, span in spans do
		if truncated then
			break
		end
		local character = span.character
		if character == "\n" then
			flushWord()
			if truncated then
				break
			end
			pushLine()
			local currentHeight = #lines * lineAdvance
			if truncate and maxHeight ~= nil and currentHeight > maxHeight then
				while #lines * lineAdvance > maxHeight and #lines > 0 do
					table.remove(lines)
				end
				truncated = true
				performTruncation()
			end
		elseif character == " " then
			flushWord()
			if currentLineWidth > 0 then
				currentLineWidth += spaceWidth
			end
		else
			local idx = font:charIndex(character) or 0
			table.insert(currentWord, {
				character = character,
				index = idx,
				attrs = span.attrs,
			})
		end
	end

	if not truncated then
		flushWord()
		if not truncated and #currentLineWords > 0 then
			pushLine()
		end
	end

	return finalizeLayout(font, lines, options, scale, lineAdvance, letterSpacing, truncated)
end

return Layout
