--!strict
-- Moddable rich-text parser (platform-agnostic).

local Utf8Util = require(script.Parent.utf8_util)

export type RichTextSpan = {
	character: string,
	attrs: { [string]: any },
}

export type TagHandler = {
	onOpen: ((tag: string, attrs: { [string]: string }, state: { [string]: any }) -> ())?,
	onClose: ((tag: string, state: { [string]: any }) -> ())?,
}

local TagRegistry = {
	handlers = {} :: { [string]: TagHandler },
}

local function parseHexColor(hex: string): { r: number, g: number, b: number }?
	local clean = hex
	if string.sub(clean, 1, 1) == "#" then
		clean = string.sub(clean, 2)
	end
	if #clean ~= 6 then
		return nil
	end
	local r = tonumber(string.sub(clean, 1, 2), 16)
	local g = tonumber(string.sub(clean, 3, 4), 16)
	local b = tonumber(string.sub(clean, 5, 6), 16)
	if r == nil or g == nil or b == nil then
		return nil
	end
	return { r = r, g = g, b = b }
end

local function parseTagAttributes(tagContent: string): (string, { [string]: string })
	local eq = string.find(tagContent, "=")
	if eq then
		local name = string.lower(string.sub(tagContent, 1, eq - 1))
		local value = string.sub(tagContent, eq + 1)
		if name == "anim" then
			return name, { name = value }
		end
		if name == "color" then
			return name, { color = value }
		end
	end

	local space = string.find(tagContent, "%s")
	if space == nil then
		return string.lower(tagContent), {}
	end
	local name = string.lower(string.sub(tagContent, 1, space - 1))
	local attrs: { [string]: string } = {}
	for key, value in string.gmatch(string.sub(tagContent, space + 1), "(%w+)%s*=%s*\"([^\"]+)\"") do
		attrs[key] = value
	end
	return name, attrs
end

local function registerDefaultTags()
	local colorHandler = {
		onOpen = function(_tag, attrs, state)
			state.colorStack = state.colorStack or {}
			if attrs.color then
				local parsed = parseHexColor(attrs.color)
				if parsed then
					table.insert(state.colorStack, parsed)
				end
			end
		end,
		onClose = function(_tag, state)
			local stack = state.colorStack
			if stack and #stack > 0 then
				table.remove(stack)
			end
		end,
	}
	TagRegistry.handlers["color"] = colorHandler
	TagRegistry.handlers["font"] = colorHandler

	TagRegistry.handlers["anim"] = {
		onOpen = function(_tag, attrs, state)
			state.animationStack = state.animationStack or {}
			local name = attrs.name or "default"
			table.insert(state.animationStack, name)
		end,
		onClose = function(_tag, state)
			local stack = state.animationStack
			if stack and #stack > 0 then
				table.remove(stack)
			end
		end,
	}
end

registerDefaultTags()

local RichText = {}

function RichText.registerTag(name: string, handler: TagHandler)
	TagRegistry.handlers[string.lower(name)] = handler
end

function RichText.currentAttrs(state: { [string]: any }): { [string]: any }
	local attrs: { [string]: any } = {}
	local colorStack = state.colorStack
	if colorStack and #colorStack > 0 then
		attrs.color = colorStack[#colorStack]
	end
	local animationStack = state.animationStack
	if animationStack and #animationStack > 0 then
		attrs.animation = animationStack[#animationStack]
		attrs.animated = true
	end
	return attrs
end

function RichText.parse(text: string): { RichTextSpan }
	local characters: { RichTextSpan } = {}
	local state: { [string]: any } = {}
	local index = 1

	while index <= #text do
		if string.sub(text, index, index) == "<" then
			local closeIndex = string.find(text, ">", index + 1, true)
			if closeIndex then
				local tagContent = string.sub(text, index + 1, closeIndex - 1)
				local lower = string.lower(tagContent)
				if string.sub(lower, 1, 1) == "/" then
					local name = string.sub(lower, 2)
					local handler = TagRegistry.handlers[name]
					if handler and handler.onClose then
						handler.onClose(name, state)
					end
				else
					local name, attrs = parseTagAttributes(tagContent)
					local handler = TagRegistry.handlers[string.lower(name)]
					if handler and handler.onOpen then
						handler.onOpen(name, attrs, state)
					end
				end
				index = closeIndex + 1
				continue
			end
		end

		local ch, nextIndex = Utf8Util.nextChar(text, index)
		if ch == nil then
			index += 1
			continue
		end

		local attrs = RichText.currentAttrs(state)
		table.insert(characters, {
			character = ch,
			attrs = attrs,
		})
		index = nextIndex
	end

	return characters
end

return RichText
