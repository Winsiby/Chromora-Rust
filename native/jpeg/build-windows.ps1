param([string]$BuildRoot=(Join-Path ([IO.Path]::GetTempPath()) ('chromora-jpeg-'+[guid]::NewGuid().ToString('N'))))
$ErrorActionPreference='Stop'
if(Test-Path -LiteralPath $BuildRoot){throw 'BuildRoot must be a new directory; existing content is never removed.'}
$archive=Join-Path $PSScriptRoot '..\libraw\sources\libjpeg-turbo-3.1.2.tar.gz'
if((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash -ne '8f0012234b464ce50890c490f18194f913a7b1f4e6a03d6644179fa0f867d0cf'){throw 'JPEG source SHA256 mismatch'}
$vswhere='C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe'
$vsRoot=& $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
Import-Module (Join-Path $vsRoot 'Common7\Tools\Microsoft.VisualStudio.DevShell.dll')
Enter-VsDevShell -VsInstallPath $vsRoot -SkipAutomaticLocation -DevCmdArguments '-arch=x64 -host_arch=x64' | Out-Null
$cmake=Join-Path $vsRoot 'Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe'
New-Item -ItemType Directory -Path $BuildRoot | Out-Null
Push-Location -LiteralPath $BuildRoot
try{& $cmake -E tar xzf $archive;if($LASTEXITCODE -ne 0){throw 'JPEG source extraction failed'}}finally{Pop-Location}
$source=Join-Path $BuildRoot 'libjpeg-turbo-3.1.2'
$build=Join-Path $BuildRoot 'jpeg-build'
& $cmake -S $source -B $build -G 'NMake Makefiles' '-DCMAKE_BUILD_TYPE=Release' '-DENABLE_SHARED=OFF' '-DENABLE_STATIC=ON' '-DWITH_CRT_DLL=ON' '-DWITH_SIMD=OFF' '-DWITH_TURBOJPEG=OFF' '-DWITH_TESTS=OFF'
if($LASTEXITCODE -ne 0){throw 'JPEG configuration failed'}
& $cmake --build $build --target jpeg-static
if($LASTEXITCODE -ne 0){throw 'JPEG build failed'}
Get-Item -LiteralPath (Join-Path $build 'jpeg-static.lib') | Select-Object FullName,Length
Get-FileHash -LiteralPath (Join-Path $build 'jpeg-static.lib') -Algorithm SHA256
