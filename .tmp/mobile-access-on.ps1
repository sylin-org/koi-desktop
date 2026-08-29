$ErrorActionPreference = 'Continue'
# 1) bind config -> 0.0.0.0 (create or patch the line)
$cfg = "$env:ProgramData\koi\config.toml"
$lines = if (Test-Path $cfg) { Get-Content $cfg } else { @() }
$found = $false
$out = foreach ($line in $lines) {
  if ($line -match '^\s*http_bind\s*=') { $found = $true; 'http_bind = "0.0.0.0"' }
  else { $line }
}
if (-not $found) { $out = @($out) + 'http_bind = "0.0.0.0"' }
$out | Set-Content $cfg
# 2) firewall rule for the daemon's UI port, scoped to the service binary
netsh advfirewall firewall delete rule name="Koi Web UI (tcp 5641)" | Out-Null
$bin = (sc.exe qc koi | Select-String 'BINARY_PATH') -replace '.*BINARY_PATH_NAME\s*:\s*', ''
$bin = ($bin -split '--')[0].Trim().Trim('"')
netsh advfirewall firewall add rule name="Koi Web UI (tcp 5641)" dir=in action=allow protocol=tcp localport=5641 program="$bin" | Out-Null
# 3) restart so the new bind takes effect
sc.exe stop koi | Out-Null
Start-Sleep -Seconds 3
sc.exe start koi | Out-Null
Start-Sleep -Seconds 3
Write-Output "mobile access enabled: bind 0.0.0.0, firewall rule scoped to $bin"
