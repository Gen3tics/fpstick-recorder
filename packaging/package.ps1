# Puts together what people download: the built recorder, Intel's PresentMon beside it (the one file, checked by its
# hash and by Intel's signature), the licences and the read-me, as FPStick-Recorder.zip. Run from the repository's
# root after "cargo build --release". Used by both workflows, so every push tries it.
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$presentmon = 'PresentMon-2.6.0-x64.exe'
$from = 'https://github.com/GameTechDev/PresentMon/releases/download/v2.6.0/PresentMon-2.6.0-x64.exe'
$sha256 = 'b2a706bc6ad475749e3b7e3409263aa1e6906d45bdcf993f6dbc0f660188f1af'
$licence = 'https://raw.githubusercontent.com/GameTechDev/PresentMon/v2.6.0/LICENSE.txt'

$d = 'FPStick Recorder'
if (Test-Path $d) { Remove-Item $d -Recurse -Force }
New-Item -ItemType Directory "$d\licences" | Out-Null

Invoke-WebRequest $from -OutFile "$d\$presentmon"
$hash = (Get-FileHash "$d\$presentmon" -Algorithm SHA256).Hash.ToLower()
if ($hash -ne $sha256) { throw "PresentMon's hash is $hash, not the one written here" }
$sig = Get-AuthenticodeSignature "$d\$presentmon"
if ($sig.Status -ne 'Valid' -or $sig.SignerCertificate.Subject -notlike '*O=Intel Corporation*') { throw "PresentMon is not signed by Intel: $($sig.Status) $($sig.SignerCertificate.Subject)" }
Invoke-WebRequest $licence -OutFile "$d\licences\PresentMon (Intel) - MIT.txt"

Copy-Item target\release\recorder.exe "$d\FPStick Recorder.exe"
Copy-Item 'packaging\READ ME FIRST.txt' $d
Copy-Item LICENSE-MIT "$d\licences\FPStick Recorder - MIT.txt"
Copy-Item LICENSE-APACHE "$d\licences\FPStick Recorder - Apache 2.0.txt"
# the libraries built into the recorder, each with the licence it is under and where its code is
"The recorder is built with these libraries. Each is under the licence named beside it; its text is in the library's own repository.`r`n" | Out-File "$d\licences\Libraries.txt" -Encoding utf8
cargo tree --edges normal --prefix none --format '{p} | {l} | {r}' --target x86_64-pc-windows-msvc | Sort-Object -Unique | Out-File "$d\licences\Libraries.txt" -Encoding utf8 -Append

# the built recorder must find the PresentMon beside it
$found = (& "$d\FPStick Recorder.exe" info | Select-String '^PresentMon:' | Select-Object -First 1).Line
"$found"
if ("$found" -notlike "*$d\$presentmon") { throw 'The recorder did not find the PresentMon beside it' }

if (Test-Path FPStick-Recorder.zip) { Remove-Item FPStick-Recorder.zip }
Compress-Archive -Path $d -DestinationPath FPStick-Recorder.zip
$zip = (Get-FileHash FPStick-Recorder.zip -Algorithm SHA256).Hash.ToLower()
"$zip  FPStick-Recorder.zip" | Out-File FPStick-Recorder.zip.sha256 -Encoding ascii
Get-ChildItem $d -Recurse -File | ForEach-Object { '{0,10}  {1}' -f $_.Length, $_.FullName.Substring((Resolve-Path $d).Path.Length + 1) }
'{0,10}  FPStick-Recorder.zip  sha256 {1}' -f (Get-Item FPStick-Recorder.zip).Length, $zip
