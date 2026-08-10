@echo off
setlocal EnableExtensions DisableDelayedExpansion

rem ==================================================================
rem Provider Deck one-click Windows release build
rem Double-click this file from the project root. No prompt is needed.
rem It creates an NSIS installer, a portable EXE, README and SHA-256 list.
rem ==================================================================

set "SCRIPT_DIR=%~dp0"
for %%I in ("%SCRIPT_DIR%.") do set "PROJECT_ROOT=%%~fI"
cd /d "%PROJECT_ROOT%"

set "PRODUCT_NAME=ProviderDeck"
set "RUNTIME_ID=win-x64"
set "BUNDLE_TYPE=nsis"
set "ARTIFACTS_DIR=%PROJECT_ROOT%\artifacts"
set "TARGET_DIR=%PROJECT_ROOT%\src-tauri\target"
set "BUILD_LOG=%ARTIFACTS_DIR%\build-all.log"
set "VERSION="
set "RELEASE_DIR="
set "INSTALLER_SOURCE="
set "ERROR_HINT=Unknown build error."

call :header
call :check_tools
if errorlevel 1 goto :failed
call :read_version
if errorlevel 1 goto :failed
call :validate_project
if errorlevel 1 goto :failed
call :prepare_output
if errorlevel 1 goto :failed
call :install_dependencies
if errorlevel 1 goto :failed
call :build_release
if errorlevel 1 goto :failed
call :collect_artifacts
if errorlevel 1 goto :failed
call :write_release_notes
if errorlevel 1 goto :failed
call :write_checksums
if errorlevel 1 goto :failed

echo.
echo ==================================================================
echo BUILD COMPLETE
echo ==================================================================
echo Version: %VERSION%
echo Release directory: "%RELEASE_DIR%"
echo Build log: "%BUILD_LOG%"
echo.
exit /b 0

:header
echo.
echo ==================================================================
echo Provider Deck one-click build and packaging
echo ==================================================================
echo Project root: %PROJECT_ROOT%
echo Target: Windows x64
echo Output: NSIS installer EXE and portable EXE
echo.
exit /b 0

:check_tools
echo [1/8] Checking Node.js, npm and Rust tools...
where.exe node >nul 2>nul
if errorlevel 1 (
    set "ERROR_HINT=Node.js was not found. Install Node.js 20 or later, then run build_all.bat again."
    exit /b 1
)
where.exe npm >nul 2>nul
if errorlevel 1 (
    set "ERROR_HINT=npm was not found. Reinstall Node.js, then run build_all.bat again."
    exit /b 1
)
where.exe cargo >nul 2>nul
if errorlevel 1 (
    set "ERROR_HINT=Rust Cargo was not found. Install Rust stable with rustup, then run build_all.bat again."
    exit /b 1
)
where.exe rustc >nul 2>nul
if errorlevel 1 (
    set "ERROR_HINT=Rust compiler was not found. Install Rust stable with rustup, then run build_all.bat again."
    exit /b 1
)
node --version
call npm --version
cargo --version
echo [OK] Required build tools are available.
echo.
exit /b 0

:read_version
echo [2/8] Reading application version...
for /f "usebackq delims=" %%V in (`node -e "console.log(require('./package.json').version)"`) do set "VERSION=%%V"
if not defined VERSION (
    set "ERROR_HINT=Could not read the version from package.json."
    exit /b 1
)
set "RELEASE_DIR=%PROJECT_ROOT%\release\ProviderDeck-%VERSION%-windows-x64"
echo Version: %VERSION%
echo.
exit /b 0

:validate_project
echo [3/8] Validating project files...
for %%F in ("%PROJECT_ROOT%\package.json" "%PROJECT_ROOT%\package-lock.json" "%PROJECT_ROOT%\src-tauri\Cargo.toml" "%PROJECT_ROOT%\src-tauri\tauri.conf.json") do (
    if not exist "%%~F" (
        set "ERROR_HINT=Required project file was not found: %%~F"
        exit /b 1
    )
)
echo [OK] Project files are complete.
echo.
exit /b 0

:prepare_output
echo [4/8] Preparing output directories...
if not exist "%ARTIFACTS_DIR%" mkdir "%ARTIFACTS_DIR%" >nul 2>nul
if exist "%BUILD_LOG%" del /q "%BUILD_LOG%"

rem Only the version-specific release directory is refreshed.
if exist "%RELEASE_DIR%" rmdir /s /q "%RELEASE_DIR%"
mkdir "%RELEASE_DIR%" >nul 2>nul
if not exist "%RELEASE_DIR%" (
    set "ERROR_HINT=Could not create release directory: %RELEASE_DIR%"
    exit /b 1
)

rem Use the repository's Cargo cache when it is present, unless caller chose another one.
if not defined CARGO_HOME if exist "%PROJECT_ROOT%\.cargo-home" set "CARGO_HOME=%PROJECT_ROOT%\.cargo-home"
set "CARGO_TARGET_DIR=%TARGET_DIR%"
echo [OK] Output directories are ready.
echo.
exit /b 0

:install_dependencies
echo [5/8] Installing locked JavaScript dependencies...
call npm ci >> "%BUILD_LOG%" 2>&1
if errorlevel 1 (
    set "ERROR_HINT=npm ci failed. Check network, npm registry and package-lock.json. See build log for details."
    exit /b 1
)
echo [OK] JavaScript dependencies are ready.
echo.
exit /b 0

:build_release
echo [6/8] Building frontend, Rust application and NSIS installer...
call npm run tauri build -- --bundles %BUNDLE_TYPE% >> "%BUILD_LOG%" 2>&1
if errorlevel 1 (
    set "ERROR_HINT=Tauri release build failed. See build log for compiler or packaging details."
    exit /b 1
)
if not exist "%TARGET_DIR%\release\provider-deck.exe" (
    set "ERROR_HINT=Portable executable was not produced: %TARGET_DIR%\release\provider-deck.exe"
    exit /b 1
)
echo [OK] Tauri release build completed.
echo.
exit /b 0

:collect_artifacts
echo [7/8] Collecting installer and portable executable...
for /f "usebackq delims=" %%I in (`powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -Command "$file = $null; foreach ($candidate in (Get-ChildItem -LiteralPath '%TARGET_DIR%\release\bundle\nsis' -Filter '*.exe' -File -ErrorAction SilentlyContinue)) { if (($null -eq $file) -or ($candidate.LastWriteTime -gt $file.LastWriteTime)) { $file = $candidate } }; if ($null -ne $file) { $file.FullName }"`) do set "INSTALLER_SOURCE=%%I"
if not defined INSTALLER_SOURCE (
    set "ERROR_HINT=NSIS installer was not found under %TARGET_DIR%\release\bundle\nsis."
    exit /b 1
)

copy /y "%INSTALLER_SOURCE%" "%RELEASE_DIR%\%PRODUCT_NAME%-Setup-%VERSION%-x64.exe" >nul
if errorlevel 1 (
    set "ERROR_HINT=Could not copy the NSIS installer to the release directory."
    exit /b 1
)
copy /y "%TARGET_DIR%\release\provider-deck.exe" "%RELEASE_DIR%\%PRODUCT_NAME%-Portable-%VERSION%-x64.exe" >nul
if errorlevel 1 (
    set "ERROR_HINT=Could not copy the portable executable to the release directory."
    exit /b 1
)
echo [OK] Release executables were collected.
echo.
exit /b 0

:write_release_notes
echo [8/8] Writing release notes and SHA-256 checksums...
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -Command ^
  "$ErrorActionPreference='Stop'; $release='%RELEASE_DIR%'; @('Provider Deck %VERSION% Windows x64 release', '', 'Files:', '- %PRODUCT_NAME%-Setup-%VERSION%-x64.exe : NSIS installer. Run this file to install Provider Deck.', '- %PRODUCT_NAME%-Portable-%VERSION%-x64.exe : portable edition. Run this file directly; installation is not required.', '', 'Before updating, exit any running Provider Deck process.', 'API keys remain in the operating system credential store and are not bundled into this release.') | Set-Content -LiteralPath (Join-Path $release 'README.txt') -Encoding utf8" >> "%BUILD_LOG%" 2>&1
if errorlevel 1 (
    set "ERROR_HINT=Could not write README.txt."
    exit /b 1
)
exit /b 0

:write_checksums
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -Command ^
  "$ErrorActionPreference='Stop'; $release='%RELEASE_DIR%'; $files=Get-ChildItem -LiteralPath $release -File | Where-Object Name -ne 'SHA256SUMS.txt'; $lines=$files | Get-FileHash -Algorithm SHA256 | ForEach-Object { '{0}  {1}' -f $_.Hash, $_.Path.Substring($release.Length + 1) }; Set-Content -LiteralPath (Join-Path $release 'SHA256SUMS.txt') -Value $lines -Encoding utf8" >> "%BUILD_LOG%" 2>&1
if errorlevel 1 (
    set "ERROR_HINT=Could not write SHA256SUMS.txt."
    exit /b 1
)
echo [OK] Release notes and checksums were written.
exit /b 0

:failed
echo.
echo ==================================================================
echo BUILD FAILED
echo ==================================================================
echo %ERROR_HINT%
echo Build log: "%BUILD_LOG%"
echo.
echo Source files and previous versioned release folders were not changed.
exit /b 1
