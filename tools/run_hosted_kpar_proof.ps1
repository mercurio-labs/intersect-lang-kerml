param(
    [ValidateSet('Demo', 'Shell')][string]$Mode = 'Demo',
    [string]$Project,
    [string]$Cache,
    [switch]$Build,
    [string]$BuildDirectory
)
$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path $PSScriptRoot -Parent
$demoRoot = Join-Path $repoRoot 'target/component-demo'
if (-not $Cache) { $Cache = Join-Path $demoRoot 'cache' }
if (-not $BuildDirectory) { $BuildDirectory = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path ([IO.Path]::GetTempPath()) 'intersect-hosted-kpar-proof' } }
$executable = Join-Path $demoRoot 'hosted_kpar_usage.exe'
if ($Build -or -not (Test-Path -LiteralPath $executable)) {
    New-Item -ItemType Directory -Path $demoRoot -Force | Out-Null
    $previousTarget = $env:CARGO_TARGET_DIR
    $previousIncremental = $env:CARGO_INCREMENTAL
    Push-Location $repoRoot
    try {
        $env:CARGO_TARGET_DIR = $BuildDirectory
        $env:CARGO_INCREMENTAL = '0'
        & cargo build --locked --profile test-fast `
            --config 'profile.test-fast.package.intersect-model.opt-level=1' `
            --config 'profile.test-fast.package.intersect-text.opt-level=1' `
            -p intersect-kpar-host --features components --example hosted_kpar_usage
        if ($LASTEXITCODE -ne 0) { throw 'Harness build failed' }
        Copy-Item -LiteralPath (Join-Path $BuildDirectory 'test-fast/examples/hosted_kpar_usage.exe') -Destination $executable
    } finally {
        if ($null -eq $previousTarget) { Remove-Item Env:CARGO_TARGET_DIR -ErrorAction SilentlyContinue } else { $env:CARGO_TARGET_DIR = $previousTarget }
        if ($null -eq $previousIncremental) { Remove-Item Env:CARGO_INCREMENTAL -ErrorAction SilentlyContinue } else { $env:CARGO_INCREMENTAL = $previousIncremental }
        Pop-Location
    }
}
$arguments = @($Cache, $(if ($Mode -eq 'Shell') { '--shell' } else { '--demo' }))
if ($Project) {
    if ($Mode -ne 'Shell') { throw '-Project is supported by -Mode Shell' }
    $arguments += (Resolve-Path -LiteralPath $Project).Path
}
if ($MyInvocation.ExpectingInput) { $input | & $executable @arguments } else { & $executable @arguments }
if ($LASTEXITCODE -ne 0) { throw "Harness exited with code $LASTEXITCODE" }
