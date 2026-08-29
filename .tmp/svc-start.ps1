sc.exe start koi | Select-String 'STATE'
Start-Sleep -Seconds 2
sc.exe query koi | Select-String 'STATE'
