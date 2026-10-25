# Windows PowerShell 5.1+ adapter. All Codex state changes use app-server RPC.
function Resolve-CodexExecutable {
    $command = Get-Command codex.exe -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($command) { return $command.Source }
    foreach ($root in @((Join-Path $env:LOCALAPPDATA 'OpenAI\Codex\bin'), (Join-Path $env:APPDATA 'npm\node_modules\@openai\codex'))) {
        if (Test-Path -LiteralPath $root) {
            $candidate = Get-ChildItem -LiteralPath $root -Filter codex.exe -Recurse -File -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -First 1
            if ($candidate) { return $candidate.FullName }
        }
    }
    throw 'Codex executable not found. Open the installed Codex app once, or provide -CodexExecutable.'
}

function ConvertTo-NativeArgument([string]$Value) {
    $text = New-Object Text.StringBuilder
    [void]$text.Append('"'); $slashes = 0
    foreach ($character in $Value.ToCharArray()) {
        if ($character -eq '\') { $slashes++; continue }
        if ($character -eq '"') {
            [void]$text.Append(('\' * (2 * $slashes + 1))); [void]$text.Append('"')
        } else {
            [void]$text.Append(('\' * $slashes)); [void]$text.Append($character)
        }
        $slashes = 0
    }
    [void]$text.Append(('\' * (2 * $slashes))); [void]$text.Append('"')
    return $text.ToString()
}

function Invoke-CodexNativeRestore {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory=$true)][string]$ThreadId,
        [Parameter(Mandatory=$true)][string]$CodexHome,
        [string]$ExpectedName = '',
        [string]$Cwd = '',
        [string]$CodexExecutable = '',
        [string[]]$StartupArguments = @('app-server'),
        [ValidateRange(1,120)][int]$TimeoutSeconds = 30,
        [switch]$AllowUnarchive
    )
    if ($ThreadId -notmatch '^[0-9a-fA-F]{8}(-[0-9a-fA-F]{4}){3}-[0-9a-fA-F]{12}$') { throw 'Invalid thread UUID.' }
    if (!(Test-Path -LiteralPath $CodexHome -PathType Container)) { throw 'Target Codex home does not exist.' }
    foreach ($unsafeTarget in @($env:USERPROFILE, $env:SystemRoot, [IO.Path]::GetPathRoot([IO.Path]::GetFullPath($CodexHome)))) {
        if ($unsafeTarget -and [IO.Path]::GetFullPath($CodexHome).TrimEnd('\') -eq [IO.Path]::GetFullPath($unsafeTarget).TrimEnd('\')) { throw 'Use a dedicated Codex home, not a user, system, or drive root.' }
    }
    if ($Cwd -and !(Test-Path -LiteralPath $Cwd -PathType Container)) { throw 'Selected working directory does not exist; no directory was created.' }
    if (!$CodexExecutable) { $CodexExecutable = Resolve-CodexExecutable }
    if (!(Test-Path -LiteralPath $CodexExecutable -PathType Leaf)) { throw 'Codex executable does not exist.' }
    $info = New-Object Diagnostics.ProcessStartInfo
    $info.FileName = $CodexExecutable
    $info.Arguments = (@($StartupArguments | ForEach-Object { ConvertTo-NativeArgument $_ }) -join ' ')
    $info.UseShellExecute = $false; $info.CreateNoWindow = $true
    $info.RedirectStandardInput = $true; $info.RedirectStandardOutput = $true; $info.RedirectStandardError = $true
    $info.StandardOutputEncoding = [Text.Encoding]::UTF8; $info.StandardErrorEncoding = [Text.Encoding]::UTF8
    $info.EnvironmentVariables['CODEX_HOME'] = [IO.Path]::GetFullPath($CodexHome)
    $process = New-Object Diagnostics.Process; $process.StartInfo = $info
    [void]$process.Start(); $stderr = $process.StandardError.ReadToEndAsync()
    function Invoke-Rpc([string]$Method, $Parameters) {
        $id = [Guid]::NewGuid().ToString('N')
        $request = @{id=$id;method=$Method;params=$Parameters} | ConvertTo-Json -Depth 12 -Compress
        # PS 5.1 may give the StreamWriter a legacy console encoding. RPC is UTF-8.
        $bytes = [Text.Encoding]::UTF8.GetBytes($request + "`n")
        $process.StandardInput.BaseStream.Write($bytes,0,$bytes.Length); $process.StandardInput.BaseStream.Flush()
        $watch = [Diagnostics.Stopwatch]::StartNew()
        while ($watch.Elapsed.TotalSeconds -lt $TimeoutSeconds) {
            $read = $process.StandardOutput.ReadLineAsync()
            $remaining = [Math]::Max(1, [int](1000 * $TimeoutSeconds - $watch.Elapsed.TotalMilliseconds))
            if (!$read.Wait($remaining)) { throw ('Codex RPC timeout: ' + $Method) }
            if ($null -eq $read.Result) { throw ('Codex exited before replying: ' + $Method) }
            try { $response = $read.Result | ConvertFrom-Json } catch { continue }
            if ($response.id -ne $id) { continue }
            if ($null -ne $response.error) { throw ('Codex RPC ' + $Method + ': ' + ($response.error | ConvertTo-Json -Compress -Depth 5)) }
            if ($null -eq $response.PSObject.Properties['result']) { throw ('Missing RPC result: ' + $Method) }
            return $response.result
        }
        throw ('Codex RPC timeout: ' + $Method)
    }
    try {
        $initialized = Invoke-Rpc 'initialize' @{clientInfo=@{name='codex_migrate_native_restore';version='1.0'};capabilities=@{experimentalApi=$true}}
        if ($initialized.codexHome -and [IO.Path]::GetFullPath($initialized.codexHome).TrimEnd('\') -ne [IO.Path]::GetFullPath($CodexHome).TrimEnd('\')) { throw 'Backend Codex home differs from the selected target.' }
        $bytes = [Text.Encoding]::UTF8.GetBytes('{"method":"initialized","params":{}}' + "`n")
        $process.StandardInput.BaseStream.Write($bytes,0,$bytes.Length); $process.StandardInput.BaseStream.Flush()
        $before = Invoke-Rpc 'thread/read' @{threadId=$ThreadId;includeTurns=$false}
        if ($before.thread.status.type -eq 'active') { throw 'Thread is active. Its files were retained; retry native restoration when it is idle.' }
        $parameters = @{threadId=$ThreadId;excludeTurns=$true}
        if ($Cwd) { $parameters.cwd = [IO.Path]::GetFullPath($Cwd) }
        $archived = $false
        try { $resumed = Invoke-Rpc 'thread/resume' $parameters }
        catch {
            if ($_.Exception.Message -notmatch '(?i)is archived|thread.*archived') { throw }
            if ($AllowUnarchive) {
                $null = Invoke-Rpc 'thread/unarchive' @{threadId=$ThreadId}
                $resumed = Invoke-Rpc 'thread/resume' $parameters
            } else {
                $archived = $true
                $resumed = Invoke-Rpc 'thread/read' @{threadId=$ThreadId;includeTurns=$false}
            }
        }
        if ($resumed.thread.id -ne $ThreadId) { throw 'Codex returned a different thread UUID.' }
        $desiredName = [string]$before.thread.name
        $nameRestored = $false
        if (!$archived -and [string]::IsNullOrWhiteSpace($desiredName) -and ![string]::IsNullOrWhiteSpace($ExpectedName)) {
            $null = Invoke-Rpc 'thread/name/set' @{threadId=$ThreadId;name=$ExpectedName}
            $desiredName = $ExpectedName; $nameRestored = $true
        }
        $page = Invoke-Rpc 'thread/turns/list' @{threadId=$ThreadId;limit=1;itemsView='notLoaded'}
        $after = Invoke-Rpc 'thread/read' @{threadId=$ThreadId;includeTurns=$false}
        if ($after.thread.id -ne $ThreadId) { throw 'Readback returned a different thread UUID.' }
        if ($desiredName -and $after.thread.name -ne $desiredName) { throw 'Thread name readback did not match; native restoration is incomplete.' }
        if ($Cwd -and !$archived -and [IO.Path]::GetFullPath($after.thread.cwd).TrimEnd('\') -ne [IO.Path]::GetFullPath($Cwd).TrimEnd('\')) { throw 'Working directory readback did not match. Use terminal resume with --cd.' }
        $count = if ($null -eq $page.data) { 0 } else { @($page.data).Count }
        $status = if ($archived) { 'archived' } elseif ($count -eq 0) { 'empty' } else { 'ready' }
        [pscustomobject]@{
            id=$ThreadId;status=$status;name=$after.thread.name;nameRestored=$nameRestored
            historyPageCount=$count;historyVerified=($count -gt 0);hasMore=[bool]$page.nextCursor
            historyVerification=$(if ($archived -and $count -eq 0) { 'deferredUntilExplicitReopen' } elseif ($count -gt 0) { 'verified' } else { 'empty' })
            archived=$archived;cwd=$after.thread.cwd;cwdExists=[bool](Test-Path -LiteralPath $after.thread.cwd -PathType Container)
            pendingName=$(if ($archived -and !$desiredName) { $ExpectedName } else { '' })
            desktopDisplayVerified=$false;modelTurnStarted=$false
        }
    } finally {
        $process.StandardInput.Close()
        if (!$process.WaitForExit(3000)) { $process.Kill(); $process.WaitForExit() }
        $process.Dispose()
    }
}

Export-ModuleMember -Function Invoke-CodexNativeRestore, Resolve-CodexExecutable
