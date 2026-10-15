<#
.SYNOPSIS
    Puts the website up (Cloudflare Pages, project glancepc), with the files
    it serves that are not kept in this repository.

.DESCRIPTION
    The phone remote downloads OpenRGB when "Control the lights" is turned
    on, from the website first: Gitee, where mainland China reaches Glance's
    releases, refuses the archive, and the project's own site is not reached
    from everywhere. The archive is another project's, as that project built
    it, and too large to keep here: it is taken from that project when it is
    not in website\dl already, checked against the digest the remote checks
    it by (remote\src\fetch.rs), and served beside its source code (it is
    under the GPL).

    Needs CLOUDFLARE_API_TOKEN and CLOUDFLARE_ACCOUNT_ID.

.EXAMPLE
    scripts\site.ps1
#>
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$root = Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $root

# The archive and its digest, as the remote names them.
$fetch = Get-Content -Raw remote\src\fetch.rs
$from = [regex]::Match($fetch, '"(https://codeberg\.org/[^"]+/(OpenRGB_[^"/]+\.zip))"')
$digest = [regex]::Match($fetch, 'const DIGEST: &str = "([0-9a-f]{64})"').Groups[1].Value
if (-not $from.Success -or -not $digest) { throw 'the archive or its digest is not named in remote\src\fetch.rs' }
$name = $from.Groups[2].Value
$source = 'OpenRGB-release_1.0-source.zip'

$served = Join-Path $root 'website\dl'
New-Item -ItemType Directory -Force $served | Out-Null
$archive = Join-Path $served $name
if (-not (Test-Path $archive)) {
    Invoke-WebRequest $from.Groups[1].Value -OutFile $archive
}
if ((Get-FileHash $archive -Algorithm SHA256).Hash -ne $digest) { throw "$name is not the archive the remote checks for" }
if (-not (Test-Path (Join-Path $served $source))) {
    Invoke-WebRequest 'https://codeberg.org/OpenRGB/OpenRGB/archive/release_1.0.zip' -OutFile (Join-Path $served $source)
}

# The latest release, as GitHub has it: its installer served from here too (Glance asks this site first for a
# newer version, and takes the installer from it: see /api/latest in website\_worker.js), and what Glance is
# told of it, in the terms GitHub and Gitee answer in. Only an installer signed as the releases are goes up.
$tag = (gh release view --repo lulu-loopp/glance --json tagName --jq .tagName).Trim()
if ($LASTEXITCODE -ne 0 -or $tag -notmatch '^v[0-9.]+$') { throw 'the latest release could not be read from GitHub' }
$installer = "Glance_$($tag.Substring(1))_x64-setup.exe"
if (-not (Test-Path (Join-Path $served $installer))) {
    gh release download $tag --repo lulu-loopp/glance --pattern $installer --dir $served --clobber
    if ($LASTEXITCODE -ne 0) { throw "$installer could not be taken from GitHub" }
}
$signed = Get-AuthenticodeSignature (Join-Path $served $installer)
if ($signed.Status -ne 'Valid') { throw "$installer is not validly signed" }
# The phone app of the release too: the app takes it in itself from here (see /api/app and Update.kt), GitHub not
# reached from everywhere. Only one signed with the app's release key goes up (the phone would refuse any other as
# an update anyway).
$app = "Glance_$($tag.Substring(1)).apk"
if (-not (Test-Path (Join-Path $served $app))) {
    gh release download $tag --repo lulu-loopp/glance --pattern $app --dir $served --clobber
    if ($LASTEXITCODE -ne 0) { throw "$app could not be taken from GitHub" }
}
$appSigner = 'ae9b1d5b1e0ff6fbaae74648af16accb2bbd708f8e99d05ecc8f21dc8644958f'
# (The Android SDK the app is built with, as android\local.properties names it.)
$sdk = [regex]::Match((Get-Content -Raw android\local.properties), '(?m)^sdk\.dir=(.+)$').Groups[1].Value.Trim()
$buildTools = Get-ChildItem (Join-Path $sdk 'build-tools') -Directory -ErrorAction SilentlyContinue | Sort-Object { [version]$_.Name } | Select-Object -Last 1
if (-not $buildTools) { throw "the Android SDK's build tools (apksigner) were not found under $sdk" }
$certificates = & (Join-Path $buildTools.FullName 'apksigner.bat') verify --print-certs (Join-Path $served $app) 2>&1
# (apksigner runs on Java: without it, it says so and checks nothing.)
if ($certificates -match 'JAVA_HOME') { throw "apksigner could not run: no Java (set JAVA_HOME to a JDK, as the app is built with)" }
if ($LASTEXITCODE -ne 0 -or -not ($certificates -match "certificate SHA-256 digest: $appSigner")) { throw "$app is not signed with the app's release key" }
# (Those of releases before are no longer served.)
Get-ChildItem $served -Filter 'Glance_*_x64-setup.exe' | Where-Object Name -ne $installer | Remove-Item
Get-ChildItem $served -Filter 'Glance_*.apk' | Where-Object Name -ne $app | Remove-Item
$told = @{ tag_name = $tag; assets = @(
    @{ name = $installer; browser_download_url = "https://glancepc.com/dl/$installer" },
    @{ name = $app; browser_download_url = "https://glancepc.com/dl/$app" }
) } | ConvertTo-Json -Depth 4 -Compress
[IO.File]::WriteAllText((Join-Path $root 'website\latest.json'), $told, (New-Object Text.UTF8Encoding $false))

foreach ($needed in 'CLOUDFLARE_API_TOKEN', 'CLOUDFLARE_ACCOUNT_ID') {
    if (-not (Get-Item "env:$needed" -ErrorAction SilentlyContinue)) { throw "$needed is not set" }
}
npx wrangler@4 pages deploy website --project-name glancepc --branch main
if ($LASTEXITCODE -ne 0) { throw 'the site was not put up' }
