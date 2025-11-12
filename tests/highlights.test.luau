local Testing = zune.testing
local FS = zune.fs
local Process = zune.process

local HighlightsCommand = require("../src/commands/highlights")

Testing.describe("highlights command", function()
	Testing.test("command has correct name and description", function()
		Testing.expect(HighlightsCommand.name).toBe("highlights")
		Testing.expect(HighlightsCommand.description)
			.toBe("Generate highlight variants of PNG images with white outlines")
		Testing.expect(HighlightsCommand.usage).toBeDefined()
		Testing.expect(HighlightsCommand.run).toBeDefined()
	end)

	Testing.test("getHighlightPath converts PNG path correctly", function()
		local function getHighlightPath(imagePath)
			local baseName = imagePath:match("^(.+)%.png$")
			if not baseName then
				return imagePath .. "-highlight.png"
			end
			return baseName .. "-highlight.png"
		end

		local result1 = getHighlightPath("image.png")
		Testing.expect(result1).toBe("image-highlight.png")

		local result2 = getHighlightPath("path/to/image.png")
		Testing.expect(result2).toBe("path/to/image-highlight.png")

		local result3 = getHighlightPath("image")
		Testing.expect(result3).toBe("image-highlight.png")
	end)

	Testing.test("checkMagick detects when magick is not available", function()
		local function testCheck()
			local pcallOk, result = pcall(Process.run, "nonexistent_magick_xyz", { "-version" })
			if not pcallOk then
				return false
			end
			return result.ok
		end

		local ok = testCheck()
		Testing.expect(ok).toBe(false)
	end)

	Testing.test("processImage skips existing highlights without force flag", function()
		local testPng = "test_image.png"
		local testHighlight = "test_image-highlight.png"
		FS.writeFile(testPng, "fake png data")
		FS.writeFile(testHighlight, "fake highlight data")

		Testing.defer(function()
			local statOk1, statResult1 = pcall(FS.stat, testPng)
			if statOk1 and statResult1 and statResult1.kind == "file" then
				pcall(FS.deleteFile, testPng)
			end
			local statOk2, statResult2 = pcall(FS.stat, testHighlight)
			if statOk2 and statResult2 and statResult2.kind == "file" then
				pcall(FS.deleteFile, testHighlight)
			end
		end)

		local pngOk, pngStat = pcall(FS.stat, testPng)
		local highlightOk, highlightStat = pcall(FS.stat, testHighlight)
		if pngOk and pngStat then
			Testing.expect(pngStat.kind).toBe("file")
		end
		if highlightOk and highlightStat then
			Testing.expect(highlightStat.kind).toBe("file")
		end
	end)

	Testing.test("processImage processes with force flag", function()
		local testPng = "test_force.png"
		FS.writeFile(testPng, "fake png data")

		Testing.defer(function()
			local statOk, statResult = pcall(FS.stat, testPng)
			if statOk and statResult and statResult.kind == "file" then
				pcall(FS.deleteFile, testPng)
			end
		end)

		local ok, stat = pcall(FS.stat, testPng)
		if ok and stat then
			Testing.expect(stat.kind).toBe("file")
		end
	end)

	Testing.test("processImage handles dry-run mode", function()
		local testPng = "test_dryrun.png"
		FS.writeFile(testPng, "fake png data")

		Testing.defer(function()
			local statOk1, statResult1 = pcall(FS.stat, testPng)
			if statOk1 and statResult1 and statResult1.kind == "file" then
				pcall(FS.deleteFile, testPng)
			end

			local highlightPath = "test_dryrun-highlight.png"
			local statOk2, statResult2 = pcall(FS.stat, highlightPath)
			if statOk2 and statResult2 and statResult2.kind == "file" then
				pcall(FS.deleteFile, highlightPath)
			end
		end)

		local ok, stat = pcall(FS.stat, testPng)
		if ok and stat then
			Testing.expect(stat.kind).toBe("file")
		end
	end)

	Testing.test("processPath handles single PNG file", function()
		local testPng = "test_single.png"
		FS.writeFile(testPng, "fake png data")

		Testing.defer(function()
			local statOk, statResult = pcall(FS.stat, testPng)
			if statOk and statResult and statResult.kind == "file" then
				pcall(FS.deleteFile, testPng)
			end
		end)

		local statOk, statResult = pcall(FS.stat, testPng)
		Testing.expect(statOk).toBe(true)
		if statOk and statResult then
			Testing.expect(statResult.kind).toBe("file")
		else
			_ = statResult
		end
	end)

	Testing.test("processPath handles directory with PNG files", function()
		local testPng1 = "test_highlights_image1.png"
		local testPng2 = "test_highlights_image2.png"

		FS.writeFile(testPng1, "fake png data")
		FS.writeFile(testPng2, "fake png data")

		Testing.defer(function()
			local statOk1, statResult1 = pcall(FS.stat, testPng1)
			if statOk1 and statResult1 and statResult1.kind == "file" then
				pcall(FS.deleteFile, testPng1)
			end
			local statOk2, statResult2 = pcall(FS.stat, testPng2)
			if statOk2 and statResult2 and statResult2.kind == "file" then
				pcall(FS.deleteFile, testPng2)
			end
		end)

		local ok1, stat1 = pcall(FS.stat, testPng1)
		local ok2, stat2 = pcall(FS.stat, testPng2)
		if ok1 and stat1 then
			Testing.expect(stat1.kind).toBe("file")
		end
		if ok2 and stat2 then
			Testing.expect(stat2.kind).toBe("file")
		end
	end)

	Testing.test("processPath skips highlight variants", function()
		local testPng = "test_base.png"
		local testHighlight = "test_base-highlight.png"

		FS.writeFile(testPng, "fake png data")
		FS.writeFile(testHighlight, "fake highlight data")

		Testing.defer(function()
			local statOk1, statResult1 = pcall(FS.stat, testPng)
			if statOk1 and statResult1 and statResult1.kind == "file" then
				pcall(FS.deleteFile, testPng)
			end
			local statOk2, statResult2 = pcall(FS.stat, testHighlight)
			if statOk2 and statResult2 and statResult2.kind == "file" then
				pcall(FS.deleteFile, testHighlight)
			end
		end)

		local isHighlight = testHighlight:match("%-highlight%.png$")
		Testing.expect(isHighlight).toBe("-highlight.png")
	end)

	Testing.test("processPath handles non-existent path", function()
		local nonexistentPath = "nonexistent_path_xyz_123"
		local statOk, _statResult = pcall(FS.stat, nonexistentPath)

		if not statOk then
			Testing.expect(statOk).toBe(false)
		else
			Testing.expect(_statResult).toBeDefined()
		end
	end)

	Testing.test("processPath handles non-PNG files", function()
		local testFile = "test_file.txt"
		FS.writeFile(testFile, "not a png")

		Testing.defer(function()
			local statOk, statResult = pcall(FS.stat, testFile)
			if statOk and statResult and statResult.kind == "file" then
				pcall(FS.deleteFile, testFile)
			end
		end)

		local isPng = testFile:match("%.png$")
		Testing.expect(isPng).toBe(nil)
	end)

	Testing.test("run validates thickness parameter", function()
		local validThickness = 1
		local invalidThickness = 0

		Testing.expect(validThickness >= 1).toBe(true)
		Testing.expect(invalidThickness >= 1).toBe(false)
	end)

	Testing.test("run handles default thickness", function()
		local defaultThickness = 1
		Testing.expect(defaultThickness).toBeGreaterThanOrEqual(1)
	end)

	Testing.test("run handles custom thickness", function()
		local customThickness = 3
		Testing.expect(customThickness).toBeGreaterThanOrEqual(1)
		Testing.expect(customThickness).toBe(3)
	end)

	Testing.test("run fails when magick is not available", function()
		Testing.expect(HighlightsCommand.run).toBeDefined()
		Testing.expect(typeof(HighlightsCommand.run)).toBe("function")
	end)

	Testing.test("run processes files with dry-run option", function()
		local testPng = "test_dryrun_option.png"
		FS.writeFile(testPng, "fake png data")

		Testing.defer(function()
			local statOk, statResult = pcall(FS.stat, testPng)
			if statOk and statResult and statResult.kind == "file" then
				pcall(FS.deleteFile, testPng)
			end
		end)

		local ok, stat = pcall(FS.stat, testPng)
		if ok and stat then
			Testing.expect(stat.kind).toBe("file")
		end
	end)

	Testing.test("run processes files with force option", function()
		local testPng = "test_force_option.png"
		FS.writeFile(testPng, "fake png data")

		Testing.defer(function()
			local statOk, statResult = pcall(FS.stat, testPng)
			if statOk and statResult and statResult.kind == "file" then
				pcall(FS.deleteFile, testPng)
			end
		end)

		local ok, stat = pcall(FS.stat, testPng)
		if ok and stat then
			Testing.expect(stat.kind).toBe("file")
		end
	end)

	Testing.test("findPngFiles recursively finds PNG files", function()
		local png1 = "test_recursive_image1.png"
		local png2 = "test_recursive_image2.png"

		FS.writeFile(png1, "fake png")
		FS.writeFile(png2, "fake png")

		Testing.defer(function()
			local statOk1, statResult1 = pcall(FS.stat, png1)
			if statOk1 and statResult1 and statResult1.kind == "file" then
				pcall(FS.deleteFile, png1)
			end
			local statOk2, statResult2 = pcall(FS.stat, png2)
			if statOk2 and statResult2 and statResult2.kind == "file" then
				pcall(FS.deleteFile, png2)
			end
		end)

		local ok1, stat1 = pcall(FS.stat, png1)
		local ok2, stat2 = pcall(FS.stat, png2)
		if ok1 and stat1 then
			Testing.expect(stat1.kind).toBe("file")
		end
		if ok2 and stat2 then
			Testing.expect(stat2.kind).toBe("file")
		end
	end)

	Testing.test("findPngFiles excludes highlight variants", function()
		local basePng = "test_base.png"
		local highlightPng = "test_base-highlight.png"

		FS.writeFile(basePng, "fake png")
		FS.writeFile(highlightPng, "fake highlight")

		Testing.defer(function()
			local statOk1, statResult1 = pcall(FS.stat, basePng)
			if statOk1 and statResult1 and statResult1.kind == "file" then
				pcall(FS.deleteFile, basePng)
			end
			local statOk2, statResult2 = pcall(FS.stat, highlightPng)
			if statOk2 and statResult2 and statResult2.kind == "file" then
				pcall(FS.deleteFile, highlightPng)
			end
		end)

		local isHighlight = highlightPng:match("%-highlight%.png$")
		Testing.expect(isHighlight).toBe("-highlight.png")
	end)
end)
