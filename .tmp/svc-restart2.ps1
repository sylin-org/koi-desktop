sc.exe stop koi | Out-Null
Start-Sleep -Seconds 4
sc.exe start koi | Out-Null
Start-Sleep -Seconds 3
sc.exe query koi | Select-String 'STATE'
