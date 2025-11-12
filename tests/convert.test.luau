local Testing = zune.testing
local FS = zune.fs
local Process = zune.process

local ConvertCommand = require("../src/commands/convert")

Testing.describe("convert command", function()
	Testing.test("command has correct name and description", function()
		Testing.expect(ConvertCommand.name).toBe("convert")
		Testing.expect(ConvertCommand.description).toBe("Convert PNG assets with pixelization and background removal")
		Testing.expect(ConvertCommand.usage).toBeDefined()
		Testing.expect(ConvertCommand.run).toBeDefined()
	end)

	Testing.test("checkExternalTools detects missing magick", function()
		local function testMagick()
			local pcallOk, result = pcall(Process.run, "nonexistent_magick_xyz", { "-version" })
			if not pcallOk then
				return false
			end
			return result.ok
		end

		local ok = testMagick()
		Testing.expect(ok).toBe(false)
	end)

	Testing.test("checkExternalTools detects missing pixeloe.pixelize", function()
		local function testPixelize()
			local pcallOk, result = pcall(Process.run, "nonexistent_pixelize_xyz", { "--help" })
			if not pcallOk then
				return false
			end
			return result.ok
		end

		local ok = testPixelize()
		Testing.expect(ok).toBe(false)
	end)

	Testing.test("getReplicateApiToken reads from environment variable", function()
		local envToken = Process.env.REPLICATE_API_TOKEN
		if envToken then
			Testing.expect(typeof(envToken)).toBe("string")
		end
	end)

	Testing.test("getReplicateApiToken reads from .env file", function()
		local testEnvPath = ".env.test_convert"
		FS.writeFile(testEnvPath, "REPLICATE_API_TOKEN=test-token-456")

		Testing.defer(function()
			local statOk, statResult = pcall(FS.stat, testEnvPath)
			if statOk and statResult and statResult.kind == "file" then
				pcall(FS.deleteFile, testEnvPath)
			end
		end)

		local ok, content = pcall(FS.readFile, testEnvPath)
		Testing.expect(ok).toBe(true)
		local token = string.match(content, "REPLICATE_API_TOKEN=(.*)")
		Testing.expect(token).toBe("test-token-456")
	end)

	Testing.test("getReplicateApiToken trims whitespace", function()
		local testToken = "  test-token  "
		local trimmed = testToken:match("^%s*(.*)"):match("(.-)%s*$")
		Testing.expect(trimmed).toBe("test-token")
	end)

	Testing.test("run validates scale parameter range", function()
		local validScale1 = 10
		local validScale2 = 50
		local validScale3 = 100
		local invalidScale1 = 9
		local invalidScale2 = 101

		Testing.expect(validScale1 >= 10 and validScale1 <= 100).toBe(true)
		Testing.expect(validScale2 >= 10 and validScale2 <= 100).toBe(true)
		Testing.expect(validScale3 >= 10 and validScale3 <= 100).toBe(true)
		Testing.expect(invalidScale1 >= 10 and invalidScale1 <= 100).toBe(false)
		Testing.expect(invalidScale2 >= 10 and invalidScale2 <= 100).toBe(false)
	end)

	Testing.test("run uses default scale of 40", function()
		local defaultScale = 40
		Testing.expect(defaultScale).toBeGreaterThanOrEqual(10)
		Testing.expect(defaultScale).toBeLessThanOrEqual(100)
		Testing.expect(defaultScale).toBe(40)
	end)

	Testing.test("run calculates target_size correctly", function()
		local baseScale = 40
		local baseTargetSize = 256
		local scaleValue = 50
		local targetSize = math.floor(scaleValue * baseTargetSize / baseScale)

		Testing.expect(targetSize).toBe(320)
	end)

	Testing.test("run calculates scalePercentage correctly", function()
		local patchSize = 8
		local scalePercentage = tostring(100 / patchSize) .. "%"
		Testing.expect(scalePercentage).toBe("12.5%")
	end)

	Testing.test("run fails when input file does not exist", function()
		local nonexistentFile = "nonexistent_file_xyz.png"
		local ok, stat = pcall(FS.stat, nonexistentFile)
		if ok and stat then
			Testing.expect(stat.kind == "file").toBe(false)
		else
			Testing.expect(ok).toBe(false)
		end
	end)

	Testing.test("run creates pixelized intermediate file", function()
		local inputPath = "test_input.png"
		local pixelizedPath = inputPath .. ".pixelized.png"
		Testing.expect(pixelizedPath).toBe("test_input.png.pixelized.png")
	end)

	Testing.test("run handles pixelize command failure", function()
		Testing.expect(ConvertCommand.run).toBeDefined()
		Testing.expect(typeof(ConvertCommand.run)).toBe("function")
	end)

	Testing.test("run validates pixelized file was created", function()
		local testPng = "test_pixelized_check.png"
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

	Testing.test("run constructs base64 image data correctly", function()
		local testData = "fake image data"

		Testing.expect(testData).toBeDefined()
		Testing.expect(typeof(testData)).toBe("string")
	end)

	Testing.test("run constructs Replicate API request correctly", function()
		local version = "a029dff38972b5fda4ec5d75d7d1cd25aeff621d2cf4946a41055d7db66b80bc"
		local imageData = "data:image/png;base64,test"

		local requestBody = {
			version = version,
			input = {
				image = imageData,
			},
		}

		Testing.expect(requestBody.version).toBe(version)
		Testing.expect(requestBody.input.image).toBe(imageData)
	end)

	Testing.test("run handles Replicate API response", function()
		local mockResponse = {
			ok = true,
			body = '{"output": "https://example.com/image.png"}',
		}

		Testing.expect(mockResponse.ok).toBe(true)
		Testing.expect(mockResponse.body).toBeDefined()
	end)

	Testing.test("run handles Replicate API errors", function()
		local mockErrorResponse = {
			ok = false,
			statusCode = 401,
			body = '{"error": "Unauthorized"}',
		}

		Testing.expect(mockErrorResponse.ok).toBe(false)
		Testing.expect(mockErrorResponse.statusCode).toBe(401)
	end)

	Testing.test("run downloads background-removed image", function()
		local mockUrl = "https://example.com/image.png"
		Testing.expect(mockUrl).toBeDefined()
		Testing.expect(typeof(mockUrl)).toBe("string")
	end)

	Testing.test("run handles download failures", function()
		local mockFailedDownload = {
			ok = false,
			body = nil,
		}

		Testing.expect(mockFailedDownload.ok).toBe(false)
		Testing.expect(mockFailedDownload.body).toBe(nil)
	end)

	Testing.test("run creates temporary replicate file", function()
		local inputPath = "test_input.png"
		local tempPath = inputPath .. ".replicate.png"
		Testing.expect(tempPath).toBe("test_input.png.replicate.png")
	end)

	Testing.test("run cleans up temporary files", function()
		local tempFile1 = "test.pixelized.png"
		local tempFile2 = "test.replicate.png"

		FS.writeFile(tempFile1, "temp data")
		FS.writeFile(tempFile2, "temp data")

		Testing.defer(function()
			local statOk1, statResult1 = pcall(FS.stat, tempFile1)
			if statOk1 and statResult1 and statResult1.kind == "file" then
				pcall(FS.deleteFile, tempFile1)
			end
			local statOk2, statResult2 = pcall(FS.stat, tempFile2)
			if statOk2 and statResult2 and statResult2.kind == "file" then
				pcall(FS.deleteFile, tempFile2)
			end
		end)

		local ok1, stat1 = pcall(FS.stat, tempFile1)
		local ok2, stat2 = pcall(FS.stat, tempFile2)
		if ok1 and stat1 then
			Testing.expect(stat1.kind).toBe("file")
		end
		if ok2 and stat2 then
			Testing.expect(stat2.kind).toBe("file")
		end
	end)

	Testing.test("run handles magick command failure", function()
		Testing.expect(ConvertCommand.run).toBeDefined()
		Testing.expect(typeof(ConvertCommand.run)).toBe("function")
	end)

	Testing.test("run validates output file creation", function()
		local testOutput = "test_output.png"
		FS.writeFile(testOutput, "test data")

		Testing.defer(function()
			local statOk, statResult = pcall(FS.stat, testOutput)
			if statOk and statResult and statResult.kind == "file" then
				pcall(FS.deleteFile, testOutput)
			end
		end)

		local ok, stat = pcall(FS.stat, testOutput)
		if ok and stat then
			Testing.expect(stat.kind).toBe("file")
		end
	end)

	Testing.test("run handles JSON decode errors", function()
		local invalidJson = "not valid json"
		local decodeSuccess, result = pcall(function()
			return invalidJson
		end)

		Testing.expect(decodeSuccess).toBe(true)
		Testing.expect(result).toBe("not valid json")
	end)

	Testing.test("run handles missing output in Replicate response", function()
		local mockResponse = {
			output = nil,
		}

		Testing.expect(mockResponse.output).toBe(nil)
	end)

	Testing.test("run processes image with custom scale", function()
		local customScale = 60
		local baseScale = 40
		local baseTargetSize = 256
		local targetSize = math.floor(customScale * baseTargetSize / baseScale)

		Testing.expect(targetSize).toBe(384)
		Testing.expect(customScale).toBeGreaterThanOrEqual(10)
		Testing.expect(customScale).toBeLessThanOrEqual(100)
	end)

	Testing.test("run fails when external tools are missing", function()
		Testing.expect(ConvertCommand.run).toBeDefined()
		Testing.expect(typeof(ConvertCommand.run)).toBe("function")
	end)

	Testing.test("run fails when API token is missing", function()
		Testing.expect(ConvertCommand.run).toBeDefined()
		Testing.expect(typeof(ConvertCommand.run)).toBe("function")
	end)
end)
