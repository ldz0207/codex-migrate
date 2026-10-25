param([string]$ModulePath, [string]$PythonExecutable, [string]$ServerScript)
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = New-Object Text.UTF8Encoding($false)
Import-Module $ModulePath -Force
try {
    Invoke-CodexNativeRestore -ThreadId $env:TEST_THREAD_ID -CodexHome $env:TEST_CODEX_HOME -ExpectedName $env:TEST_THREAD_NAME -Cwd $env:TEST_CWD -CodexExecutable $PythonExecutable -StartupArguments @('-u', $ServerScript) -DiagnosticPath $env:TEST_DIAGNOSTIC_PATH -TimeoutSeconds 1 -AllowUnarchive:($env:TEST_ALLOW_UNARCHIVE -eq '1') | ConvertTo-Json -Depth 5
} catch { [Console]::Error.WriteLine($_.Exception.Message); exit 1 }
