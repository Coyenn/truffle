local FS = zune.fs
local Net = zune.net
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

local function checkExternalTools(): (boolean, string?)
	-- Check magick
	local magickCheck = Process.run("magick", { "-version" })
	if not magickCheck.ok then
		return false, "magick (ImageMagick) is not available. Please install ImageMagick."
	end

	-- Check pixeloe.pixelize
	local pixelizeCheck = Process.run("pixeloe.pixelize", { "--help" })
	if not pixelizeCheck.ok then
		return false, "pixeloe.pixelize is not available. Please install pixeloe."
	end

	return true, nil
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

	-- Trim whitespace
	replicateApiToken = replicateApiToken:match("^%s*(.*)"):match("(.-)%s*$")
	return replicateApiToken, nil
end

local function run(inputPath: string, outputPath: string, scale: number?): boolean
	-- Validate scale
	local scaleValue = scale or 40
	if not scaleValue or scaleValue < 10 or scaleValue > 100 then
		print(string.format("[%s] ERROR: Scale must be between 10 and 100.", COMMAND_NAME))
		return false
	end

	-- Check external tools
	local toolsOk, toolsError = checkExternalTools()
	if not toolsOk then
		print(string.format("[%s] ERROR: %s", COMMAND_NAME, toolsError))
		return false
	end

	-- Get API token
	local replicateApiToken, tokenError = getReplicateApiToken()
	if not replicateApiToken then
		print(string.format("[%s] ERROR: %s", COMMAND_NAME, tokenError))
		return false
	end

	-- Validate input file exists
	if not FS.isFile(inputPath) then
		print(string.format("[%s] ERROR: Input file '%s' does not exist.", COMMAND_NAME, inputPath))
		return false
	end

	-- Calculate target_size based on scale with downscaling factor of 12.5 (for patch_size 8)
	-- Formula: target_size = scale * base_factor where base_factor = 256/40 = 6.4
	-- This ensures scale=40% gives target_size=256 (the original default)
	local patchSize = 8
	local baseScale = 40 -- Default scale value
	local baseTargetSize = 256 -- Default target_size for baseScale
	local targetSize = math.floor(scaleValue * baseTargetSize / baseScale)
	local scalePercentage = tostring(100 / patchSize) .. "%"

	-- Pixelize the image before background removal
	local pixelizedPath = inputPath .. ".pixelized.png"
	print(
		string.format("[%s] Pixelizing image (target_size: %d, patch_size: %d)...", COMMAND_NAME, targetSize, patchSize)
	)

	local pixelizeResult = Process.run("pixeloe.pixelize", {
		inputPath,
		"--output_img",
		pixelizedPath,
		"--target_size",
		tostring(targetSize),
		"--patch_size",
		tostring(patchSize),
	})

	if not pixelizeResult.ok then
		print(string.format("[%s] ERROR: pixeloe.pixelize command failed.", COMMAND_NAME))
		if pixelizeResult.stderr ~= "" then
			print(pixelizeResult.stderr)
		end
		return false
	end

	if not FS.isFile(pixelizedPath) then
		print(string.format("[%s] ERROR: Pixelized image was not created.", COMMAND_NAME))
		return false
	end

	-- Use pixelized image for background removal
	local pixelizedImageData = FS.readFile(pixelizedPath)
	local base64Image = Base64.encode(buffer.fromstring(pixelizedImageData))

	local requestBody = Serde.encode("json", {
		version = "a029dff38972b5fda4ec5d75d7d1cd25aeff621d2cf4946a41055d7db66b80bc",
		input = {
			image = "data:image/png;base64," .. buffer.tostring(base64Image),
		},
	})

	print(string.format("[%s] Requesting background removal from Replicate...", COMMAND_NAME))

	local response = Net.request({
		method = "POST",
		url = "https://api.replicate.com/v1/predictions",
		headers = {
			["Authorization"] = "Bearer " .. replicateApiToken,
			["Content-Type"] = "application/json",
			["Prefer"] = "wait",
		},
		body = requestBody,
	})

	if not response.ok then
		print(
			string.format(
				"[%s] ERROR: Replicate API request failed with status %s",
				COMMAND_NAME,
				tostring(response.statusCode)
			)
		)
		if response.body then
			print(response.body)
		end
		return false
	end

	local decodeSuccess, replicatePayload = pcall(function()
		return Serde.decode("json", response.body or "")
	end)
	if not decodeSuccess then
		print(string.format("[%s] ERROR: Failed to decode Replicate response JSON.", COMMAND_NAME))
		return false
	end

	local responseData = replicatePayload.output

	if not responseData then
		print(string.format("[%s] ERROR: Replicate response does not contain any outputs.", COMMAND_NAME))
		return false
	end

	print(string.format("[%s] Downloading background-removed image from Replicate...", COMMAND_NAME))

	local downloadResponse = Net.request(responseData)

	if not downloadResponse.ok or not downloadResponse.body then
		print(string.format("[%s] ERROR: Failed to download background-removed image.", COMMAND_NAME))
		return false
	end

	local tempInputPath = inputPath .. ".replicate.png"
	FS.writeFile(tempInputPath, downloadResponse.body)

	local finalInputPath = tempInputPath

	if not FS.isFile(finalInputPath) then
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

	-- Clean up temporary files
	if FS.isFile(tempInputPath) then
		FS.deleteFile(tempInputPath)
	end
	if FS.isFile(pixelizedPath) then
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
