@echo off
setlocal EnableExtensions DisableDelayedExpansion

rem ==================================================================
rem Provider Deck one-click Windows release build
rem Double-click this file from the project root. No prompt is needed.
rem It creates an NSIS installer, a portable EXE, README, release
rem summary and SHA-256 list under release\ProviderDeck-<version>-windows-x64.
rem
rem Chinese release documents are written by scripts\write-release-docs.mjs
rem because the console code page would garble UTF-8 text echoed from here.
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
rem Version probe output lands in TEMP: :read_version runs before :prepare_output
rem creates the artifacts directory.
set "ARTIFACTS_DIR_TMP=%TEMP%\provider-deck-version-%RANDOM%.txt"
set "VERSION="
set "RELEASE_DIR="
set "INSTALLER_SOURCE="
set "EXE_FRESHNESS="
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
call :run_tests
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
echo Portable: %PRODUCT_NAME%-Portable-%VERSION%-x64.exe (single file, needs WebView2 Runtime)
echo Installer: %PRODUCT_NAME%-Setup-%VERSION%-x64.exe (NSIS, start menu shortcut, uninstall entry)
echo Docs: README.txt, release-summary.txt, SHA256SUMS.txt
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
echo [1/10] Checking Node.js, npm and Rust tools...
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
echo [2/10] Reading and cross-checking the application version...
rem check-version.mjs compares package.json, Cargo.toml and tauri.conf.json.
rem It prints the version only when all three agree, otherwise it exits non-zero
rem and the mismatch is reported below. Never hardcode the version here.
node "%PROJECT_ROOT%\scripts\check-version.mjs" > "%ARTIFACTS_DIR_TMP%" 2>&1
if errorlevel 1 (
    echo.
    type "%ARTIFACTS_DIR_TMP%"
    del /q "%ARTIFACTS_DIR_TMP%" >nul 2>nul
    set "ERROR_HINT=Version numbers disagree between package.json, src-tauri/Cargo.toml and src-tauri/tauri.conf.json."
    exit /b 1
)
for /f "usebackq delims=" %%V in ("%ARTIFACTS_DIR_TMP%") do set "VERSION=%%V"
del /q "%ARTIFACTS_DIR_TMP%" >nul 2>nul
if not defined VERSION (
    set "ERROR_HINT=Could not read the version from package.json."
    exit /b 1
)
set "RELEASE_DIR=%PROJECT_ROOT%\release\ProviderDeck-%VERSION%-windows-x64"
echo [OK] package.json, Cargo.toml and tauri.conf.json all report %VERSION%.
echo.
exit /b 0

:validate_project
echo [3/10] Validating project files...
for %%F in ("%PROJECT_ROOT%\package.json" "%PROJECT_ROOT%\package-lock.json" "%PROJECT_ROOT%\src-tauri\Cargo.toml" "%PROJECT_ROOT%\src-tauri\tauri.conf.json" "%PROJECT_ROOT%\scripts\check-version.mjs" "%PROJECT_ROOT%\scripts\write-release-docs.mjs") do (
    if not exist "%%~F" (
        set "ERROR_HINT=Required project file was not found: %%~F"
        exit /b 1
    )
)
echo [OK] Project files are complete.
echo.
exit /b 0

:prepare_output
echo [4/10] Preparing output directories...
if not exist "%ARTIFACTS_DIR%" mkdir "%ARTIFACTS_DIR%" >nul 2>nul
if exist "%BUILD_LOG%" del /q "%BUILD_LOG%" >nul 2>nul

rem Every step below redirects into the build log. If another build_all.bat is
rem still running it holds that log open, the redirects fail, the commands never
rem execute - and errorlevel stays 0, so the script would print [OK] for tests
rem and packaging it never ran. Refuse to start unless the log is writable.
if exist "%BUILD_LOG%" (
    set "ERROR_HINT=Could not clear the build log, so another build_all.bat is probably still running. Wait for it to finish, then run build_all.bat again: %BUILD_LOG%"
    exit /b 1
)
echo Build started at %DATE% %TIME% > "%BUILD_LOG%" 2>nul
if not exist "%BUILD_LOG%" (
    set "ERROR_HINT=Could not create the build log: %BUILD_LOG%"
    exit /b 1
)

rem Only the version-specific release directory is refreshed. Wiping it whole
rem is what keeps a stale EXE from an earlier build of the same version out of
rem the release folder.
if exist "%RELEASE_DIR%" rmdir /s /q "%RELEASE_DIR%" >nul 2>nul

rem rmdir cannot delete an EXE that is currently running, and it fails per-file
rem without setting errorlevel. Testing the directory alone is not enough: a
rem partially deleted directory still exists, so the build would carry a stale
rem portable EXE into the release. Require the directory to be fully gone.
if exist "%RELEASE_DIR%" (
    set "ERROR_HINT=Could not empty the release directory. A Provider Deck instance launched from it is probably still running - exit Provider Deck, then run build_all.bat again: %RELEASE_DIR%"
    exit /b 1
)
mkdir "%RELEASE_DIR%" >nul 2>nul
if not exist "%RELEASE_DIR%" (
    set "ERROR_HINT=Could not create release directory: %RELEASE_DIR%"
    exit /b 1
)

rem Clear the NSIS bundle staging directory too. It is not version-scoped, so a
rem setup EXE from an earlier release stays behind; :collect_artifacts picks the
rem newest file there and would silently ship that old installer if this run's
rem bundle step never produced one.
if exist "%TARGET_DIR%\release\bundle\nsis" rmdir /s /q "%TARGET_DIR%\release\bundle\nsis" >nul 2>nul
if exist "%TARGET_DIR%\release\bundle\nsis" (
    set "ERROR_HINT=Could not empty the NSIS staging directory. Close any running Provider Deck installer, then run build_all.bat again: %TARGET_DIR%\release\bundle\nsis"
    exit /b 1
)

rem Use the repository's Cargo cache when it is present, unless caller chose another one.
if not defined CARGO_HOME if exist "%PROJECT_ROOT%\.cargo-home" set "CARGO_HOME=%PROJECT_ROOT%\.cargo-home"
set "CARGO_TARGET_DIR=%TARGET_DIR%"
echo [OK] Output directories are ready.
echo.
exit /b 0

:install_dependencies
echo [5/10] Installing locked JavaScript dependencies...
call npm ci >> "%BUILD_LOG%" 2>&1
if errorlevel 1 (
    set "ERROR_HINT=npm ci failed. Check network, npm registry and package-lock.json. See build log for details."
    exit /b 1
)

rem Do not trust errorlevel alone here. npm ci wipes node_modules before it
rem refills it, and if a stale process holds a native module the unlink fails
rem with EPERM partway through - leaving a gutted node_modules while the batch
rem layer still sees success. Everything downstream then runs against missing
rem binaries: npx silently fetches an unrelated package named "tsc" from the
rem registry and the type check fails for the wrong reason. Verify the tools
rem this build actually needs are present on disk.
for %%B in (tsc.cmd vitest.cmd eslint.cmd playwright.cmd tauri.cmd) do (
    if not exist "%PROJECT_ROOT%\node_modules\.bin\%%B" (
        set "ERROR_HINT=node_modules is incomplete - %%B is missing from node_modules\.bin. npm ci reported success but left the tree half-installed, usually because a running Provider Deck, tauri build or editor held a file. Close them and run build_all.bat again."
        exit /b 1
    )
)
echo [OK] JavaScript dependencies are ready.
echo.
exit /b 0

:run_tests
echo [6/10] Running Rust, TypeScript and frontend tests...
echo   - cargo test --lib
call cargo test --lib --manifest-path "%PROJECT_ROOT%\src-tauri\Cargo.toml" >> "%BUILD_LOG%" 2>&1
if errorlevel 1 (
    set "ERROR_HINT=cargo test --lib failed. Nothing was packaged. See build log."
    exit /b 1
)
rem Call the local binaries directly instead of going through npx. npx falls back
rem to downloading a same-named package from the registry when the local one is
rem not resolvable, which turns a missing dependency into a confusing failure
rem from somebody else's code.
echo   - tsc --noEmit
call "%PROJECT_ROOT%\node_modules\.bin\tsc.cmd" --noEmit -p "%PROJECT_ROOT%\tsconfig.app.json" >> "%BUILD_LOG%" 2>&1
if errorlevel 1 (
    set "ERROR_HINT=TypeScript type check failed. Nothing was packaged. See build log."
    exit /b 1
)
echo   - vitest run
call "%PROJECT_ROOT%\node_modules\.bin\vitest.cmd" run >> "%BUILD_LOG%" 2>&1
if errorlevel 1 (
    set "ERROR_HINT=vitest failed. Nothing was packaged. See build log."
    exit /b 1
)
echo   - eslint
call npm run lint >> "%BUILD_LOG%" 2>&1
if errorlevel 1 (
    set "ERROR_HINT=eslint reported problems. Nothing was packaged. See build log."
    exit /b 1
)
echo [OK] All test gates passed.
echo.
exit /b 0

:build_release
echo [7/10] Building frontend, Rust application and NSIS installer...

rem Drop a marker immediately before the build. Checking only that the EXE
rem exists is not enough: the previous release leaves one behind, so a build
rem that never ran would still pass an existence test and ship a stale binary.
rem The EXE has to be newer than this marker to count as this run's output.
echo marker > "%ARTIFACTS_DIR%\build-marker.tmp"

call npm run tauri build -- --bundles %BUNDLE_TYPE% >> "%BUILD_LOG%" 2>&1
if errorlevel 1 (
    set "ERROR_HINT=Tauri release build failed. See build log for compiler or packaging details."
    exit /b 1
)
if not exist "%TARGET_DIR%\release\provider-deck.exe" (
    set "ERROR_HINT=Portable executable was not produced: %TARGET_DIR%\release\provider-deck.exe"
    exit /b 1
)

rem Compare against the marker written just before the build.
for /f "usebackq delims=" %%S in (`powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -Command "$exe = Get-Item -LiteralPath '%TARGET_DIR%\release\provider-deck.exe'; $marker = Get-Item -LiteralPath '%ARTIFACTS_DIR%\build-marker.tmp'; if ($exe.LastWriteTime -ge $marker.LastWriteTime) { 'fresh' } else { 'stale' }"`) do set "EXE_FRESHNESS=%%S"
del /q "%ARTIFACTS_DIR%\build-marker.tmp" >nul 2>nul
if not "%EXE_FRESHNESS%"=="fresh" (
    set "ERROR_HINT=provider-deck.exe is older than this build, so the compile did not actually run. Refusing to package a stale binary. See build log."
    exit /b 1
)
echo [OK] Tauri release build completed.
echo.
exit /b 0

:collect_artifacts
echo [8/10] Collecting installer and portable executable...
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
echo [9/10] Writing README.txt and release-summary.txt...
call node "%PROJECT_ROOT%\scripts\write-release-docs.mjs" "%RELEASE_DIR%" "%VERSION%" >> "%BUILD_LOG%" 2>&1
if errorlevel 1 (
    set "ERROR_HINT=Could not write README.txt and release-summary.txt. See build log."
    exit /b 1
)
if not exist "%RELEASE_DIR%\README.txt" (
    set "ERROR_HINT=README.txt is missing from the release directory."
    exit /b 1
)
if not exist "%RELEASE_DIR%\release-summary.txt" (
    set "ERROR_HINT=release-summary.txt is missing from the release directory."
    exit /b 1
)
echo [OK] Release documents were written.
echo.
exit /b 0

:write_checksums
rem Hash the files that actually ship, after every other file is already in
rem place, so the digests always describe this run's output.
echo [10/10] Generating SHA-256 checksums...
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -Command ^
  "$ErrorActionPreference='Stop'; $release='%RELEASE_DIR%'; $files=Get-ChildItem -LiteralPath $release -File | Where-Object Name -ne 'SHA256SUMS.txt'; $lines=$files | Get-FileHash -Algorithm SHA256 | ForEach-Object { '{0}  {1}' -f $_.Hash, $_.Path.Substring($release.Length + 1) }; Set-Content -LiteralPath (Join-Path $release 'SHA256SUMS.txt') -Value $lines -Encoding utf8" >> "%BUILD_LOG%" 2>&1
if errorlevel 1 (
    set "ERROR_HINT=Could not write SHA256SUMS.txt."
    exit /b 1
)
echo [OK] SHA256SUMS.txt was written.
echo.
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
