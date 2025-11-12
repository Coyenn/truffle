local FS = zune.fs
local Process = zune.process
local Luau = zune.luau

local COMMAND_NAME = "sync"
local COMMAND_DESCRIPTION = "Sync assets and augment metadata with image dimensions"

local function printUsage()
	print("Usage: sync")
	print("")
	print("Description:")
	print("  Syncs assets using asphalt and augments asset metadata with image dimensions")
	print("  and highlight variant IDs.")
	print("")
	print("Requirements:")
	print("  - ASPHALT_API_KEY environment variable must be set")
	print("  - asphalt command must be available")
end

local function checkAsphalt(): (boolean, string?)
	local asphaltCheck = Process.run("asphalt", { "--version" })
	if not asphaltCheck.ok then
		local asphaltCheck2 = Process.run("asphalt", {})
		if not asphaltCheck2.ok then
			return false, "asphalt command is not available. Please install asphalt."
		end
	end
	return true, nil
end

local function getAsphaltApiKey(): (string?, string?)
	local success, envFile = pcall(FS.readFile, ".env")
	local envContent = ""
	if success then
		envContent = envFile
	end

	local asphaltApiKey = Process.env.ASPHALT_API_KEY or string.match(envContent, "ASPHALT_API_KEY=(.*)")

	if not asphaltApiKey then
		return nil, "ASPHALT_API_KEY environment variable is not set. Not syncing assets."
	end

	return asphaltApiKey, nil
end

local function getPngDimensions(pngPath: string): (number?, number?)
	local ok, data = pcall(FS.readFile, pngPath)
	if not ok then
		return nil, nil
	end

	if typeof(data) == "table" then
		data = string.char(table.unpack(data))
	end

	if data:sub(1, 8) ~= "\137PNG\13\10\26\10" then
		return nil, nil
	end

	local function readUInt32BE(str: string, offset: number): number
		local b1, b2, b3, b4 = string.byte(str, offset, offset + 3)
		return ((b1 * 256 + b2) * 256 + b3) * 256 + b4
	end

	local width = readUInt32BE(data, 17)
	local height = readUInt32BE(data, 21)
	return width, height
end

local function loadAssetsTable(modulePath: string): {}
	local src = FS.readFile(modulePath)
	local compileOk, bytecode = pcall(Luau.compile, src)
	if not compileOk then
		error("Failed to compile assets module: " .. tostring(bytecode))
	end
	local loadOk, chunk = pcall(Luau.load, bytecode, {
		env = {},
		chunk_name = "@" .. modulePath,
	})
	if not loadOk then
		error("Failed to load assets module: " .. tostring(chunk))
	end
	local ok, tbl = pcall(chunk)
	if not ok then
		error("Assets module threw an error: " .. tbl)
	end
	return tbl :: {}
end

local function serialiseLuau(tbl: {}, indent: string): string
	indent = indent or ""
	local pieces = { "{" }
	local firstLevel = indent == ""
	local innerIndent = indent .. "\t"

	local keys = {}
	for k in pairs(tbl) do
		table.insert(keys, k)
	end
	table.sort(keys, function(a, b)
		return tostring(a) < tostring(b)
	end)

	for _, key in ipairs(keys) do
		local value = tbl[key]
		local linePrefix = innerIndent
		if typeof(key) == "string" and key:match("^%a[%w_]*$") then
			linePrefix ..= key .. " = "
		else
			linePrefix ..= string.format("[%q] = ", key)
		end

		if typeof(value) == "table" then
			linePrefix ..= serialiseLuau(value, innerIndent)
			linePrefix ..= ","
			table.insert(pieces, linePrefix)
		elseif typeof(value) == "string" then
			linePrefix ..= string.format("%q,", value)
			table.insert(pieces, linePrefix)
		elseif typeof(value) == "number" then
			linePrefix ..= tostring(value) .. ","
			table.insert(pieces, linePrefix)
		elseif value == nil then
		else
			error("Unsupported value type in assets table: " .. typeof(value))
		end
	end

	table.insert(pieces, indent .. "}")
	if firstLevel then
		return table.concat(pieces, "\n") .. "\n"
	else
		return table.concat(pieces, "\n")
	end
end

local function serialiseDts(tbl: {}, indent: string): string
	indent = indent or "    "
	local pieces = { "{" }
	local innerIndent = indent .. "    "

	local function quoteKey(key: any): string
		if typeof(key) == "string" and key:match("^%a[%w_]*$") then
			return key
		end
		return string.format("%q", tostring(key))
	end

	local keys = {}
	for k in pairs(tbl) do
		table.insert(keys, k)
	end
	table.sort(keys, function(a, b)
		return tostring(a) < tostring(b)
	end)

	for _, key in ipairs(keys) do
		local value = tbl[key]
		if typeof(value) == "table" then
			if value.id ~= nil then
				table.insert(pieces, string.format("%s%s: AssetMeta;", innerIndent, quoteKey(key)))
			else
				table.insert(
					pieces,
					string.format("%s%s: %s", innerIndent, quoteKey(key), serialiseDts(value, innerIndent))
				)
			end
		elseif value == nil then
		else
			table.insert(pieces, string.format("%s%s: AssetMeta;", innerIndent, quoteKey(key)))
		end
	end

	table.insert(pieces, indent .. "}")
	return table.concat(pieces, "\n")
end

local function syncAsphalt(): boolean
	local result = Process.run("asphalt", {
		"sync",
	})

	if not result.ok then
		print(string.format("[%s] ERROR: Failed to run asphalt sync: %s", COMMAND_NAME, result.stderr))
		return false
	end
	return true
end

local function run(): boolean
	local asphaltOk, asphaltError = checkAsphalt()
	if not asphaltOk then
		print(string.format("[%s] ERROR: %s", COMMAND_NAME, asphaltError))
		return false
	end

	local asphaltApiKey, keyError = getAsphaltApiKey()
	if not asphaltApiKey then
		print(string.format("[%s] WARN: %s", COMMAND_NAME, keyError))
		return false
	end

	local ASSETS_LUAU_PATH = "src/shared/data/assets/assets.luau"
	local ASSETS_DTS_PATH = "src/shared/data/assets/assets.d.ts"
	local RAW_ASSETS_FOLDER = "assets/images"

	print(string.format("[%s] Running asphalt sync …", COMMAND_NAME))
	if not syncAsphalt() then
		return false
	end

	print(string.format("[%s] Augmenting with image dimensions …", COMMAND_NAME))
	local newAssets = loadAssetsTable(ASSETS_LUAU_PATH)

	local function isAssetMetaTable(value: any): boolean
		return typeof(value) == "table" and (value.id ~= nil or value.width ~= nil or value.height ~= nil)
	end

	local function pathJoin(parts: { string }): string
		return table.concat(parts, "/")
	end

	local function getHighlightAssetId(node: any, pathSegments: { string }): string?
		local lastSegment = pathSegments[#pathSegments]
		if not lastSegment or lastSegment:match("%-highlight%.png$") then
			return nil
		end

		local highlightPath = { table.unpack(pathSegments) }
		highlightPath[#highlightPath] = lastSegment:gsub("%.png$", "-highlight.png")

		local highlightNode = newAssets
		for _, segment in ipairs(highlightPath) do
			if typeof(highlightNode) == "table" then
				highlightNode = highlightNode[segment]
			else
				return nil
			end
		end

		if typeof(highlightNode) == "string" or typeof(highlightNode) == "number" then
			return tostring(highlightNode)
		elseif isAssetMetaTable(highlightNode) then
			return tostring(highlightNode.id or highlightNode)
		end

		return nil
	end

	local function augmentNode(node: any, pathSegments: { string }): any
		if typeof(node) == "string" or typeof(node) == "number" then
			local imagePath = RAW_ASSETS_FOLDER .. "/" .. pathJoin(pathSegments)
			local w, h = getPngDimensions(imagePath)
			if not w then
				print(
					string.format(
						"[%s] WARN: %s is not a PNG or is unreadable – skipping size metadata.",
						COMMAND_NAME,
						imagePath
					)
				)
			end

			local result: { id: string, width: number, height: number, highlightId: string? } = {
				id = tostring(node),
				width = w or 0,
				height = h or 0,
			}

			local highlightId = getHighlightAssetId(node, pathSegments)
			if highlightId then
				result.highlightId = highlightId
			end

			return result
		elseif isAssetMetaTable(node) then
			local imagePath = RAW_ASSETS_FOLDER .. "/" .. pathJoin(pathSegments)
			local w, h = getPngDimensions(imagePath)
			if not w then
				print(
					string.format(
						"[%s] WARN: %s is not a PNG or is unreadable – skipping size metadata.",
						COMMAND_NAME,
						imagePath
					)
				)
			end
			local idValue = node.id or node

			local result: { id: string, width: number, height: number, highlightId: string? } = {
				id = tostring(idValue),
				width = w or node.width or 0,
				height = h or node.height or 0,
			}

			local highlightId = getHighlightAssetId(node, pathSegments)
			if highlightId then
				result.highlightId = highlightId
			end

			return result
		elseif typeof(node) == "table" then
			local result = {}
			local keys = {}
			for k in pairs(node) do
				table.insert(keys, k)
			end
			table.sort(keys, function(a, b)
				return tostring(a) < tostring(b)
			end)
			for _, k in ipairs(keys) do
				local childPath = { table.unpack(pathSegments) }
				table.insert(childPath, tostring(k))
				result[k] = augmentNode(node[k], childPath)
			end
			return result
		else
			error("Unsupported value type in assets table: " .. typeof(node))
		end
	end

	local augmentedAssets = {}
	for category, node in pairs(newAssets) do
		augmentedAssets[category] = augmentNode(node, { tostring(category) })
	end

	print(string.format("[%s] Writing augmented Luau module …", COMMAND_NAME))
	local luauOut = "-- This file is automatically @generated by sync-assets.luau.\n"
		.. "-- DO NOT EDIT MANUALLY.\n\n"
		.. "local assets = "
		.. serialiseLuau(augmentedAssets, "")
		.. "\nreturn {\n\tassets = assets\n}\n"
	FS.writeFile(ASSETS_LUAU_PATH, luauOut)

	print(string.format("[%s] Writing TypeScript declaration …", COMMAND_NAME))
	local dtsOut = [[// This file is automatically @generated by sync-assets.luau.
// DO NOT EDIT MANUALLY.

export interface AssetMeta {
    id: string;
    width: number;
    height: number;
    highlightId?: string;
}

declare const assets: ]] .. serialiseDts(augmentedAssets, "") .. "\n\nexport { assets };\n"

	FS.writeFile(ASSETS_DTS_PATH, dtsOut)

	print(string.format("[%s] Done ✅", COMMAND_NAME))
	return true
end

return {
	name = COMMAND_NAME,
	description = COMMAND_DESCRIPTION,
	usage = printUsage,
	run = run,
}
