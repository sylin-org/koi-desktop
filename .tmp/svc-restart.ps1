sc.exe stop koi | Out-Null
Start-Sleep -Seconds 3
sc.exe start koi | Select-String 'STATE'
