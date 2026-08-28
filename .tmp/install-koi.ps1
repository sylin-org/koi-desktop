$ErrorActionPreference = 'Continue'
Set-Location 'F:\Replica\NAS\Files\repo\github\sylin-org\koi'
& '.\target\release\koi.exe' install
Start-Sleep -Seconds 3
sc.exe query koi | Select-String 'STATE'
