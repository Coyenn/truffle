local FS = zune.fs
local Process = zune.process

local COMMAND_NAME = "highlights"
local COMMAND_DESCRIPTION = "Generate highlight variants of PNG images with white outlines"

local function printUsage()
	print("Usage: highlights <input-path> [options]")
	print("")
	print("Options:")
	print("  --dry-run          Preview what would be generated without creating files")
	print("  --force            Overwrite existing highlight variants")
	print("  --thickness <n>    Outline thickness in pixels (default: 1)")
	print("")
	print("Examples:")
	print("  highlights assets/images/character/")
	print("  highlights assets/images/character/base.png")
	print("  highlights assets/images/ --dry-run")
end

local function checkMagick(): (boolean, string?)
	local magickCheck = Process.run("magick", { "-version" })
	if not magickCheck.ok then
		return false, "magick (ImageMagick) is not available. Please install ImageMagick."
	end
	return true, nil
end

local function getHighlightPath(imagePath: string): string
	local baseName = imagePath:match("^(.+)%.png$")
	if not baseName then
		return imagePath .. "-highlight.png"
	end
	return baseName .. "-highlight.png"
end

local function processImage(imagePath: string, dryRun: boolean, force: boolean, thickness: number): boolean
	local highlightPath = getHighlightPath(imagePath)

	local highlightExists = false
	local statOk, statResult = pcall(FS.stat, highlightPath)
	if statOk and statResult.kind == "file" then
		highlightExists = true
	end

	if highlightExists and not force then
		print(string.format("[%s] SKIP: %s (highlight already exists)", COMMAND_NAME, imagePath))
		return false
	end

	if dryRun then
		print(string.format("[%s] DRY-RUN: Would generate %s", COMMAND_NAME, highlightPath))
		return true
	end

	print(string.format("[%s] Processing: %s", COMMAND_NAME, imagePath))

	local magickArgs = {
		imagePath,
		"-write",
		"mpr:original",
		"+delete",
		"(",
		"mpr:original",
		"-alpha",
		"extract",
		"-bordercolor",
		"black",
		"-border",
		"" .. thickness,
		"-morphology",
		"EdgeIn",
		"Diamond:" .. tostring(thickness),
		"-shave",
		"" .. thickness,
		"-write",
		"mpr:outline-mask",
		"+delete",
		")",
		"(",
		"mpr:original",
		"-alpha",
		"off",
		"-fill",
		"white",
		"-colorize",
		"100",
		"-channel",
		"A",
		"mpr:outline-mask",
		"-compose",
		"CopyOpacity",
		"-composite",
		"+channel",
		"-write",
		"mpr:white-outline",
		"+delete",
		")",
		"mpr:original",
		"mpr:white-outline",
		"-compose",
		"Over",
		"-composite",
		"-filter",
		"Point",
		highlightPath,
	}

	local result = Process.run("magick", magickArgs)

	if not result.ok then
		print(string.format("[%s] ERROR: Failed to generate highlight for %s", COMMAND_NAME, imagePath))
		if result.stderr ~= "" then
			print(result.stderr)
		end
		return false
	end

	print(string.format("[%s] ✅ Generated: %s", COMMAND_NAME, highlightPath))
	return true
end

local function processPath(path: string, dryRun: boolean, force: boolean, thickness: number): number
	local processed = 0
	local skipped = 0
	local errors = 0

	local pathStatOk, pathStat = pcall(FS.stat, path)
	if not pathStatOk then
		print(string.format("[%s] ERROR: Path does not exist: %s", COMMAND_NAME, path))
		return 0
	end

	if pathStat.kind == "file" then
		if not path:match("%.png$") then
			print(string.format("[%s] ERROR: Input must be a PNG file: %s", COMMAND_NAME, path))
			return 0
		end
		if processImage(path, dryRun, force, thickness) then
			processed = processed + 1
		else
			skipped = skipped + 1
		end
	else
		local ok, _ = pcall(FS.readDir, path)
		if ok then
			local function findPngFiles(dir: string, files: { string })
				local dirEntries = FS.readDir(dir)
				for _, entry in ipairs(dirEntries) do
					local fullPath = dir .. "/" .. entry
					local entryStatOk, entryStat = pcall(FS.stat, fullPath)
					if entryStatOk then
						local entryIsFile = entryStat.kind == "file"
						if not entryIsFile then
							findPngFiles(fullPath, files)
						elseif entry:match("%.png$") and not entry:match("%-highlight%.png$") then
							table.insert(files, fullPath)
						end
					end
				end
			end

			local pngFiles = {}
			findPngFiles(path, pngFiles)

			if #pngFiles == 0 then
				print(string.format("[%s] No PNG files found in: %s", COMMAND_NAME, path))
				return 0
			end

			print(string.format("[%s] Found %d PNG file(s) to process", COMMAND_NAME, #pngFiles))

			for _, file in ipairs(pngFiles) do
				local success = processImage(file, dryRun, force, thickness)
				if success then
					processed = processed + 1
				else
					local highlightPath = getHighlightPath(file)
					local highlightStatOk, highlightStat = pcall(FS.stat, highlightPath)
					if highlightStatOk and highlightStat.kind == "file" then
						skipped = skipped + 1
					else
						errors = errors + 1
					end
				end
			end
		else
			print(string.format("[%s] ERROR: Path does not exist: %s", COMMAND_NAME, path))
			return 0
		end
	end

	if dryRun then
		print(string.format("[%s] DRY-RUN: Would process %d file(s)", COMMAND_NAME, processed))
	else
		print(
			string.format(
				"[%s] Done ✅ Processed: %d, Skipped: %d, Errors: %d",
				COMMAND_NAME,
				processed,
				skipped,
				errors
			)
		)
	end

	return processed
end

local function run(inputPath: string, options: { dryRun: boolean?, force: boolean?, thickness: number? }): number
	local thickness = options.thickness or 1
	if not thickness or thickness < 1 then
		print(string.format("[%s] ERROR: Thickness must be >= 1", COMMAND_NAME))
		return 0
	end

	local magickOk, magickError = checkMagick()
	if not magickOk then
		print(string.format("[%s] ERROR: %s", COMMAND_NAME, magickError))
		return 0
	end

	local dryRun = options.dryRun or false
	local force = options.force or false

	return processPath(inputPath, dryRun, force, thickness)
end

return {
	name = COMMAND_NAME,
	description = COMMAND_DESCRIPTION,
	usage = printUsage,
	run = run,
}
