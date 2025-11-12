local Process = zune.process

local ConvertCommand = require("./commands/convert")
local HighlightsCommand = require("./commands/highlights")
local SyncCommand = require("./commands/sync")

local COMMANDS = {
	[ConvertCommand.name] = ConvertCommand,
	[HighlightsCommand.name] = HighlightsCommand,
	[SyncCommand.name] = SyncCommand,
}

local function printTopLevelHelp()
	print("Usage: truffle <command> [arguments]")
	print("")
	print("Commands:")
	for _, cmd in pairs(COMMANDS) do
		print(string.format("  %-15s %s", cmd.name, cmd.description))
	end
	print("")
	print("Options:")
	print("  --help, -h      Show help for a command")
	print("")
	print("Examples:")
	print("  truffle convert input.png output.png")
	print("  truffle highlights assets/images/ --dry-run")
	print("  truffle sync")
	print("  truffle convert --help")
end

local function parseConvertArgs(args: { string }): (string?, string?, number?, string?)
	if #args < 2 then
		return nil, nil, nil, "convert requires at least 2 arguments: <input.png> <output.png>"
	end

	local inputPath = tostring(args[1])
	local outputPath = tostring(args[2])
	local scale: number? = nil

	if #args >= 3 then
		scale = tonumber(args[3])
		if not scale then
			return nil, nil, nil, "scale must be a number"
		end
	end

	return inputPath, outputPath, scale, nil
end

local function checkMagick(): (boolean, string?)
	local pcallOk, magickCheck = pcall(Process.run, "magick", { "-version" })
	if not pcallOk or not magickCheck.ok then
		return false, "magick (ImageMagick) is not available. Please install ImageMagick."
	end
	return true, nil
end

local function parseHighlightsArgs(
	args: { string }
): (string?, { dryRun: boolean?, force: boolean?, thickness: number? }, string?)
	if #args < 1 then
		return nil, {}, "highlights requires at least 1 argument: <input-path>"
	end

	local inputPath = tostring(args[1])
	local options: { dryRun: boolean?, force: boolean?, thickness: number? } = {}
	local i = 2

	while i <= #args do
		local arg = args[i]
		if arg == "--dry-run" then
			options.dryRun = true
			i = i + 1
		elseif arg == "--force" then
			options.force = true
			i = i + 1
		elseif arg == "--thickness" then
			if i + 1 <= #args then
				local thickness = tonumber(args[i + 1])
				if not thickness then
					return nil, {}, "--thickness requires a numeric value"
				end
				options.thickness = thickness
				i = i + 2
			else
				return nil, {}, "--thickness requires a value"
			end
		else
			return nil, {}, string.format("Unknown option: %s", arg)
		end
	end

	return inputPath, options, nil
end

local function main()
	local args = Process.args

	-- Handle no arguments or help flag
	if #args == 0 or #args == 1 or args[2] == "--help" or args[2] == "-h" then
		printTopLevelHelp()
		Process.exit(0)
	end

	local commandName = args[2]

	-- Handle command-specific help
	if commandName == "--help" or commandName == "-h" then
		printTopLevelHelp()
		Process.exit(0)
	end

	-- Check if command exists
	local command = COMMANDS[commandName]
	if not command then
		print(string.format("Unknown command: %s", commandName))
		print("")
		printTopLevelHelp()
		Process.exit(1)
	end

	-- Handle command help
	if #args >= 3 and (args[3] == "--help" or args[3] == "-h") then
		command.usage()
		Process.exit(0)
	end

	-- Route to command
	local commandArgs = {}
	for i = 3, #args do
		table.insert(commandArgs, args[i])
	end

	local success = false

	if commandName == "convert" then
		-- Check magick availability
		local magickOk, magickError = checkMagick()
		if not magickOk then
			print(string.format("[%s] ERROR: %s", commandName, magickError))
			Process.exit(1)
		end

		local inputPath, outputPath, scale, error = parseConvertArgs(commandArgs)
		if error then
			print(string.format("[%s] ERROR: %s", commandName, error))
			command.usage()
			Process.exit(1)
		end
		success = command.run(inputPath :: string, outputPath :: string, scale)
	elseif commandName == "highlights" then
		-- Check magick availability
		local magickOk, magickError = checkMagick()
		if not magickOk then
			print(string.format("[%s] ERROR: %s", commandName, magickError))
			Process.exit(1)
		end
		local inputPath, options, error = parseHighlightsArgs(commandArgs)
		if error then
			print(string.format("[%s] ERROR: %s", commandName, error))
			command.usage()
			Process.exit(1)
		end
		local processed =
			command.run(inputPath :: string, options :: { dryRun: boolean?, force: boolean?, thickness: number? })
		success = processed > 0 or #commandArgs == 0
	elseif commandName == "sync" then
		if #commandArgs > 0 then
			print(string.format("[%s] ERROR: sync does not accept any arguments", commandName))
			command.usage()
			Process.exit(1)
		end
		success = command.run()
	else
		print(string.format("Internal error: command '%s' not properly handled", commandName))
		Process.exit(1)
	end

	if not success then
		Process.exit(1)
	end
end

main()
