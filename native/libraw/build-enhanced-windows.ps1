param([string]$BuildRoot=(Join-Path ([IO.Path]::GetTempPath()) ('chromora-libraw-'+[guid]::NewGuid().ToString('N'))))
$ErrorActionPreference='Stop'
if(Test-Path -LiteralPath $BuildRoot){throw 'BuildRoot must be a new directory; existing content is never removed.'}
$archives=Join-Path $PSScriptRoot 'sources'
$librawZip=Join-Path $archives 'LibRaw-0.22.3-Win64.zip'
$jpegTar=Join-Path $archives 'libjpeg-turbo-3.1.2.tar.gz'
if((Get-FileHash -LiteralPath $librawZip -Algorithm SHA256).Hash -ne '4e2213a0b10e32199a30b9114a61376cdb40f94df861262b4a6dca659701276b'){throw 'LibRaw source archive SHA256 mismatch'}
if((Get-FileHash -LiteralPath $jpegTar -Algorithm SHA256).Hash -ne '8f0012234b464ce50890c490f18194f913a7b1f4e6a03d6644179fa0f867d0cf'){throw 'libjpeg-turbo source archive SHA256 mismatch'}
$vswhere='C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe'
$vsRoot=& $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
Import-Module (Join-Path $vsRoot 'Common7\Tools\Microsoft.VisualStudio.DevShell.dll')
Enter-VsDevShell -VsInstallPath $vsRoot -SkipAutomaticLocation -DevCmdArguments '-arch=x64 -host_arch=x64' | Out-Null
$cmake=Join-Path $vsRoot 'Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe'
New-Item -ItemType Directory -Path $BuildRoot | Out-Null
Expand-Archive -LiteralPath $librawZip -DestinationPath $BuildRoot
Push-Location -LiteralPath $BuildRoot
try{& $cmake -E tar xzf $jpegTar;if($LASTEXITCODE -ne 0){throw 'JPEG source extraction failed'}}finally{Pop-Location}
$jpegSource=Join-Path $BuildRoot 'libjpeg-turbo-3.1.2'
$jpegBuild=Join-Path $BuildRoot 'jpeg-build'
& $cmake -S $jpegSource -B $jpegBuild -G 'NMake Makefiles' '-DCMAKE_BUILD_TYPE=Release' '-DENABLE_SHARED=OFF' '-DENABLE_STATIC=ON' '-DWITH_CRT_DLL=ON' '-DWITH_SIMD=OFF' '-DWITH_TURBOJPEG=OFF' '-DWITH_TESTS=OFF'
if($LASTEXITCODE -ne 0){throw 'JPEG configuration failed'}
& $cmake --build $jpegBuild --target jpeg-static
if($LASTEXITCODE -ne 0){throw 'JPEG build failed'}
$librawSource=Join-Path $BuildRoot 'LibRaw-0.22.3'
$jpegDef='/DUSE_JPEG /I"'+(Join-Path $jpegSource 'src')+'" /I"'+$jpegBuild+'"'
$jpegLib='"'+(Join-Path $jpegBuild 'jpeg-static.lib')+'"'
Push-Location -LiteralPath $librawSource
try{& nmake.exe /nologo /f Makefile.msvc 'bin\libraw.dll' 'COPT_OPT=/DUSE_X3FTOOLS' ('JPEG_DEF='+$jpegDef) ('JPEG_LIB='+$jpegLib);if($LASTEXITCODE -ne 0){throw 'LibRaw build failed'}}finally{Pop-Location}
Get-Item -LiteralPath (Join-Path $librawSource 'bin\libraw.dll') | Select-Object FullName,Length
Get-FileHash -LiteralPath (Join-Path $librawSource 'bin\libraw.dll') -Algorithm SHA256
