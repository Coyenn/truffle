local FS = zune.fs
local Process = zune.process
local Serde = zune.serde
local Base64 = require("../lib/base64")

local COMMAND_NAME = "convert"
local COMMAND_DESCRIPTION = "Convert PNG assets with pixelization and background removal"

local function printUsage()
	print("Usage: convert <input.png> <output.png> [scale]")
	print("")
	print("Arguments:")
	print("  input.png    Input PNG image file")
	print("  output.png   Output PNG image file")
	print("  scale        Scale percentage (10-100, default: 40)")
	print("")
	print("Examples:")
	print("  convert input.png output.png")
	print("  convert input.png output.png 50")
end

local function findPixelizeCommand(): ({ executable: string, useModule: boolean }?, string?)
	local directOk, directResult = pcall(Process.run, "pixeloe.pixelize", { "--help" })
	if directOk and directResult and directResult.ok then
		return { executable = "pixeloe.pixelize", useModule = false }, nil
	end

	local venvPath = ".venv/bin/pixeloe.pixelize"
	local statOk, stat = pcall(FS.stat, venvPath)
	if statOk and stat and stat.kind == "file" then
		return { executable = venvPath, useModule = false }, nil
	end

	return nil, "pixeloe.pixelize is not available. Please install pixeloe."
end

local function checkExternalTools(): (boolean, string?, { executable: string, useModule: boolean }?)
	local magickCheck = Process.run("magick", { "-version" })
	if not magickCheck.ok then
		return false, "magick (ImageMagick) is not available. Please install ImageMagick.", nil
	end

	local pixelizeCmd, pixelizeError = findPixelizeCommand()
	if not pixelizeCmd then
		return false, pixelizeError, nil
	end

	return true, nil, pixelizeCmd
end

local function getReplicateApiToken(): (string?, string?)
	local success, envFile = pcall(FS.readFile, ".env")
	local envContent = ""
	if success then
		envContent = envFile
	end

	local replicateApiToken = Process.env.REPLICATE_API_TOKEN or string.match(envContent, "REPLICATE_API_TOKEN=(.*)")

	if not replicateApiToken then
		return nil, "REPLICATE_API_TOKEN environment variable is not set."
	end

	replicateApiToken = replicateApiToken:match("^%s*(.*)"):match("(.-)%s*$")
	return replicateApiToken, nil
end

local function requestWithUrl(url: string, options: { [string]: any }?): (boolean, any?, string?)
	local curlCheck = Process.run("curl", { "--version" })
	if not curlCheck.ok then
		return false, nil, "curl is not available. Please install curl."
	end

	local method = "GET"
	local headers = {}
	local body = nil

	if options then
		if options.method then
			method = options.method
		end

		if options.headers then
			for headerKey, headerValue in pairs(options.headers) do
				headers[headerKey] = headerValue
			end
		end

		if options.body then
			body = typeof(options.body) == "buffer" and buffer.tostring(options.body) or tostring(options.body)
		end
	end

	local curlArgs = {
		"-s",
		"-S",
		"-w",
		"%{http_code}",
		"-X",
		method,
	}

	for headerKey, headerValue in pairs(headers) do
		table.insert(curlArgs, "-H")
		table.insert(curlArgs, string.format("%s: %s", headerKey, headerValue))
	end

	local tempBodyFile = nil
	if body and (method == "POST" or method == "PUT" or method == "PATCH") then
		tempBodyFile = "/tmp/truffle_curl_body_" .. tostring(math.random(1000000, 9999999))
		FS.writeFile(tempBodyFile, body)
		table.insert(curlArgs, "--data-binary")
		table.insert(curlArgs, "@" .. tempBodyFile)
	end

	local tempResponseFile = "/tmp/truffle_curl_response_" .. tostring(math.random(1000000, 9999999))
	table.insert(curlArgs, "-o")
	table.insert(curlArgs, tempResponseFile)

	table.insert(curlArgs, url)

	local curlResult = Process.run("curl", curlArgs)

	if tempBodyFile then
		pcall(FS.deleteFile, tempBodyFile)
	end

	if not curlResult.ok then
		pcall(FS.deleteFile, tempResponseFile)
		local errorMsg = curlResult.stderr or "Unknown curl error"
		return false, nil, errorMsg
	end

	local statusCode = 0
	local statusStr = curlResult.stdout:match("%d+")
	if statusStr then
		statusCode = tonumber(statusStr) or 0
	else
		statusCode = 200
	end

	local responseBody = ""
	local readOk, bodyData = pcall(FS.readFile, tempResponseFile)
	if readOk and bodyData then
		responseBody = bodyData
	end

	pcall(FS.deleteFile, tempResponseFile)

	local ok = statusCode >= 200 and statusCode < 300

	local response = {
		ok = ok,
		status_code = statusCode,
		status_reason = "",
		body = responseBody,
	}

	return true, response, nil
end

local function run(inputPath: string, outputPath: string, scale: number?): boolean
	local scaleValue = scale or 40
	if not scaleValue or scaleValue < 10 or scaleValue > 100 then
		print(string.format("[%s] ERROR: Scale must be between 10 and 100.", COMMAND_NAME))
		return false
	end

	local toolsOk, toolsError, pixelizeCmd = checkExternalTools()
	if not toolsOk then
		print(string.format("[%s] ERROR: %s", COMMAND_NAME, toolsError))
		return false
	end

	local replicateApiToken, tokenError = getReplicateApiToken()
	if not replicateApiToken then
		print(string.format("[%s] ERROR: %s", COMMAND_NAME, tokenError))
		return false
	end

	if FS.stat(inputPath).kind ~= "file" then
		print(string.format("[%s] ERROR: Input file '%s' does not exist.", COMMAND_NAME, inputPath))
		return false
	end

	local patchSize = 8
	local baseScale = 40
	local baseTargetSize = 256
	local targetSize = math.floor(scaleValue * baseTargetSize / baseScale)
	local scalePercentage = tostring(100 / patchSize) .. "%"

	local pixelizedPath = inputPath .. ".pixelized.png"
	print(
		string.format("[%s] Pixelizing image (target_size: %d, patch_size: %d)...", COMMAND_NAME, targetSize, patchSize)
	)

	local pixelizeArgs = {
		inputPath,
		"--output_img",
		pixelizedPath,
		"--target_size",
		tostring(targetSize),
		"--patch_size",
		tostring(patchSize),
	}

	local pixelizeOk, pixelizeResult = pcall(Process.run, pixelizeCmd.executable, pixelizeArgs)
	if not pixelizeOk then
		print(
			string.format(
				"[%s] ERROR: Failed to execute pixeloe.pixelize at '%s': %s",
				COMMAND_NAME,
				pixelizeCmd.executable,
				tostring(pixelizeResult)
			)
		)
		return false
	end

	if not pixelizeResult.ok then
		print(string.format("[%s] ERROR: pixeloe.pixelize command failed.", COMMAND_NAME))
		if pixelizeResult.stderr and pixelizeResult.stderr ~= "" then
			print(pixelizeResult.stderr)
		end
		return false
	end

	local pixelizedStatOk, pixelizedStat = pcall(FS.stat, pixelizedPath)
	if not pixelizedStatOk or not pixelizedStat or pixelizedStat.kind ~= "file" then
		print(string.format("[%s] ERROR: Pixelized image was not created.", COMMAND_NAME))
		return false
	end

	local pixelizedImageData = FS.readFile(pixelizedPath)
	local base64Image = Base64.encode(buffer.fromstring(pixelizedImageData))

	local requestBodyJson = Serde.json.encode({
		version = "a029dff38972b5fda4ec5d75d7d1cd25aeff621d2cf4946a41055d7db66b80bc",
		input = {
			image = "data:image/png;base64," .. buffer.tostring(base64Image),
		},
	})

	local headers = {
		["Authorization"] = replicateApiToken:match("^Bearer ") and replicateApiToken
			or ("Bearer " .. replicateApiToken),
		["Content-Type"] = "application/json",
		["Prefer"] = "wait",
	}

	print(string.format("[%s] Requesting background removal from Replicate...", COMMAND_NAME))

	local requestOk, response, requestError = requestWithUrl("https://api.replicate.com/v1/predictions", {
		method = "POST",
		headers = headers,
		body = requestBodyJson,
	})

	if not requestOk then
		print(
			string.format("[%s] ERROR: Failed to make HTTP request: %s", COMMAND_NAME, requestError or "Unknown error")
		)
		return false
	end

	if not response.ok then
		print(
			string.format(
				"[%s] ERROR: Replicate API request failed with status %s",
				COMMAND_NAME,
				tostring(response.status_code)
			)
		)
		if response.body then
			print(response.body)
		end
		return false
	end

	local decodeSuccess, replicatePayload = pcall(function()
		return Serde.json.decode(response.body or "")
	end)
	if not decodeSuccess then
		print(string.format("[%s] ERROR: Failed to decode Replicate response JSON.", COMMAND_NAME))
		return false
	end

	local responseData = replicatePayload.output
	if typeof(responseData) == "table" then
		responseData = responseData[1]
	end

	if typeof(responseData) ~= "string" or responseData == "" then
		print(string.format("[%s] ERROR: Replicate response does not contain any outputs.", COMMAND_NAME))
		return false
	end

	print(string.format("[%s] Downloading background-removed image from Replicate...", COMMAND_NAME))

	local downloadOk, downloadResponse, downloadError = requestWithUrl(responseData, nil)

	if not downloadOk then
		print(
			string.format(
				"[%s] ERROR: Failed to download background-removed image: %s",
				COMMAND_NAME,
				downloadError or "Unknown error"
			)
		)
		return false
	end

	if not downloadResponse.ok or not downloadResponse.body then
		print(string.format("[%s] ERROR: Failed to download background-removed image.", COMMAND_NAME))
		return false
	end

	local tempInputPath = inputPath .. ".replicate.png"
	FS.writeFile(tempInputPath, downloadResponse.body)

	local finalInputPath = tempInputPath

	local statOk, statResult = pcall(FS.stat, finalInputPath)
	if not statOk or not statResult or statResult.kind ~= "file" then
		print(string.format("[%s] ERROR: Input file '%s' does not exist.", COMMAND_NAME, finalInputPath))
		return false
	end

	print(string.format("[%s] Applying image processing...", COMMAND_NAME))

	local result = Process.run("magick", {
		finalInputPath,
		"(",
		"+clone",
		"-write",
		"mpr:orig",
		"+delete",
		")",
		"(",
		"+clone",
		"-alpha",
		"extract",
		"-morphology",
		"erode",
		"disk:1",
		"-blur",
		"0x0.7",
		"-level",
		"5%,100%",
		"-write",
		"mpr:mask",
		"+delete",
		")",
		"(",
		"+clone",
		"-alpha",
		"off",
		"-channel",
		"RGB",
		"mpr:mask",
		"-compose",
		"Multiply",
		"-composite",
		"+channel",
		")",
		"-delete",
		"0",
		"mpr:mask",
		"-compose",
		"CopyOpacity",
		"-composite",
		"-filter",
		"Point",
		"-resize",
		scalePercentage,
		"(",
		"mpr:orig",
		"-filter",
		"Point",
		"-resize",
		scalePercentage,
		"-write",
		"mpr:orig-small",
		"+delete",
		")",
		"(",
		"+clone",
		"-alpha",
		"extract",
		"-threshold",
		"70%",
		"-write",
		"mpr:alpha-cut",
		"+delete",
		")",
		"(",
		"mpr:alpha-cut",
		"-morphology",
		"Erode",
		"Diamond:1",
		"-write",
		"mpr:core",
		"+delete",
		")",
		"(",
		"mpr:alpha-cut",
		"mpr:core",
		"-compose",
		"MinusSrc",
		"-composite",
		"-threshold",
		"0.1%",
		"-write",
		"mpr:border",
		"+delete",
		")",
		"-channel",
		"RGB",
		"mpr:alpha-cut",
		"-compose",
		"Multiply",
		"-composite",
		"+channel",
		"-channel",
		"A",
		"mpr:alpha-cut",
		"-compose",
		"CopyOpacity",
		"-composite",
		"+channel",
		"(",
		"mpr:orig-small",
		"-alpha",
		"set",
		"-channel",
		"A",
		"mpr:core",
		"-compose",
		"CopyOpacity",
		"-composite",
		"+channel",
		")",
		"-compose",
		"Over",
		"-composite",
		"(",
		"+clone",
		"-channel",
		"RGB",
		"-evaluate",
		"multiply",
		"0.5",
		"+channel",
		"-alpha",
		"set",
		"-channel",
		"A",
		"mpr:border",
		"-compose",
		"CopyOpacity",
		"-composite",
		"+channel",
		")",
		"-compose",
		"Over",
		"-composite",
		"-trim",
		"+repage",
		outputPath,
	})

	local tempStatOk, tempStat = pcall(FS.stat, tempInputPath)
	if tempStatOk and tempStat and tempStat.kind == "file" then
		FS.deleteFile(tempInputPath)
	end
	local cleanupPixelizedStatOk, cleanupPixelizedStat = pcall(FS.stat, pixelizedPath)
	if cleanupPixelizedStatOk and cleanupPixelizedStat and cleanupPixelizedStat.kind == "file" then
		FS.deleteFile(pixelizedPath)
	end

	if not result.ok then
		print(string.format("[%s] ERROR: magick command failed.", COMMAND_NAME))
		if result.stderr ~= "" then
			print(result.stderr)
		end
		return false
	end

	if result.stdout ~= "" then
		print(result.stdout)
	end

	print(string.format("[%s] Done ✅ Wrote '%s'.", COMMAND_NAME, outputPath))
	return true
end

return {
	name = COMMAND_NAME,
	description = COMMAND_DESCRIPTION,
	usage = printUsage,
	run = run,
}
