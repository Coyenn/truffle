--!strict
-- Truffle Text: platform-agnostic bitmap font layout.

local Font = require(script.Parent.font)
local Layout = require(script.Parent.layout)
local RichText = require(script.Parent.rich_text)

export type FontMetaV2 = Font.FontMetaV2
export type FontHandle = Font.FontHandle
export type LayoutOptions = Layout.LayoutOptions
export type LayoutGlyph = Layout.LayoutGlyph
export type LayoutResult = Layout.LayoutResult
export type RichTextSpan = RichText.RichTextSpan
export type TagHandler = RichText.TagHandler

local TruffleText = {}

-- roblox-ts calls exported module functions with `:`; accept both `fn(x)` and `fn(self, x)`.
local function rbxtsFirst(metaOrSelf: any, meta: any?): any
	return if meta ~= nil then meta else metaOrSelf
end

local function rbxtsThree(selfOrA: any, b: any?, c: any?, d: any?): (any, any, any?)
	if d ~= nil then
		return b, c, d
	end
	return selfOrA, b, c
end

local function rbxtsTwo(selfOrA: any, b: any?, c: any?): (any, any)
	if c ~= nil then
		return b, c
	end
	return selfOrA, b
end

function TruffleText.loadFont(metaOrSelf: any, meta: FontMetaV2?): FontHandle
	return Font.fromMeta(rbxtsFirst(metaOrSelf, meta) :: FontMetaV2)
end

function TruffleText.measureText(
	selfOrFont: any,
	fontOrText: any?,
	textOrOptions: any?,
	optionsMaybe: LayoutOptions?
): { width: number, height: number }
	local font, text, options = rbxtsThree(selfOrFont, fontOrText, textOrOptions, optionsMaybe)
	return Layout.measureText(font, text, options)
end

function TruffleText.layoutText(
	selfOrFont: any,
	fontOrText: any?,
	textOrOptions: any?,
	optionsMaybe: LayoutOptions?
): LayoutResult
	local font, text, options = rbxtsThree(selfOrFont, fontOrText, textOrOptions, optionsMaybe)
	return Layout.layoutText(font, text, options)
end

function TruffleText.layoutRichText(
	selfOrFont: any,
	fontOrSpans: any?,
	spansOrOptions: any?,
	optionsMaybe: LayoutOptions?
): LayoutResult
	local font, spans, options = rbxtsThree(selfOrFont, fontOrSpans, spansOrOptions, optionsMaybe)
	return Layout.layoutRichText(font, spans, options)
end

function TruffleText.parseRichText(textOrSelf: any, text: string?): { RichTextSpan }
	return RichText.parse(rbxtsFirst(textOrSelf, text) :: string)
end

function TruffleText.registerTag(nameOrSelf: any, nameOrHandler: any?, handlerMaybe: TagHandler?)
	local name, handler = rbxtsTwo(nameOrSelf, nameOrHandler, handlerMaybe)
	RichText.registerTag(name :: string, handler :: TagHandler)
end

return TruffleText
