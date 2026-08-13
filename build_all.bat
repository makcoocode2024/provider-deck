@echo off
setlocal EnableExtensions DisableDelayedExpansion

rem ==================================================================
rem Provider Deck one-click Windows release build
rem Double-click this file from the project root. No prompt is needed.
rem It creates an NSIS installer, a portable EXE, README, release
rem summary and SHA-256 list under release\ProviderDeck-<version>-windows-x64.
rem
rem This script is the ONLY place that bumps the version. Running it performs
rem a formal release, so the patch number is incremented automatically once the
rem test gates pass (see :bump_version). Day-to-day development never touches
rem the version. Set PROVIDER_DECK_SKIP_BUMP=1 to rebuild the same version -
rem useful when a previous run failed after the bump.
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
rem Version probe output lands in TEMP: :read_version runs before :prepare_log
rem creates the artifacts directory.
rem
rem Three separate files, not one reused path. DisableDelayedExpansion freezes
rem %RANDOM% at startup, so a single variable would give all three steps the same
rem filename - and if any step failed to clean up, the next one would silently
rem read the previous step's leftover output as its own result.
set "VERSION_PROBE_FILE=%TEMP%\provider-deck-version-%RANDOM%.txt"
set "BUMP_OUT_FILE=%TEMP%\provider-deck-bumped-%RANDOM%.txt"
set "RECHECK_FILE=%TEMP%\provider-deck-recheck-%RANDOM%.txt"
set "CHANGELOG_SOURCE=%PROJECT_ROOT%\CHANGELOG.md"
set "VERSION="
set "VERSION_OLD="
set "BUMPED="
set "RELEASE_DIR="
set "INSTALLER_SOURCE="
set "EXE_FRESHNESS="
set "ERROR_HINT=Unknown build error."

rem Step order matters in two places.
rem
rem :prepare_log runs before the tests because every test step redirects into
rem the build log, so the log has to exist first. But the release directory is
rem named after the version, and the version is not final until :bump_version
rem has run - so directory creation is split out into :prepare_release_dir and
rem deferred until after the bump.
rem
rem :bump_version sits after :run_tests on purpose. Bumping first would burn a
rem version number on every failed test run and leave the working tree dirty
rem with a version that never shipped. Gates green, then bump.
call :header
call :check_tools
if errorlevel 1 goto :failed
call :read_version
if errorlevel 1 goto :failed
call :validate_project
if errorlevel 1 goto :failed
call :prepare_log
if errorlevel 1 goto :failed
call :install_dependencies
if errorlevel 1 goto :failed
call :run_tests
if errorlevel 1 goto :failed
call :bump_version
if errorlevel 1 goto :failed
call :prepare_release_dir
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
if defined BUMPED (
    echo Version: %VERSION%  ^(bumped from %VERSION_OLD%^)
) else (
    echo Version: %VERSION%  ^(bump skipped via PROVIDER_DECK_SKIP_BUMP^)
)
echo Release directory: "%RELEASE_DIR%"
echo Build log: "%BUILD_LOG%"
echo.
echo Portable: %PRODUCT_NAME%-Portable-%VERSION%-x64.exe (single file, needs WebView2 Runtime)
echo Installer: %PRODUCT_NAME%-Setup-%VERSION%-x64.exe (NSIS, start menu shortcut, uninstall entry)
echo Docs: README.txt, CHANGELOG.md, release-summary.txt, SHA256SUMS.txt
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
echo [1/12] Checking Node.js, npm and Rust tools...
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
echo [2/12] Reading and cross-checking the application version...
rem check-version.mjs compares package.json, Cargo.toml and tauri.conf.json.
rem It prints the version only when all three agree, otherwise it exits non-zero
rem and the mismatch is reported below. Never hardcode the version here.
rem
rem This reads the version the repository currently sits on. It is NOT the version
rem that ships: :bump_version increments it later and refreshes VERSION.
node "%PROJECT_ROOT%\scripts\check-version.mjs" > "%VERSION_PROBE_FILE%" 2>&1
if errorlevel 1 (
    echo.
    type "%VERSION_PROBE_FILE%"
    del /q "%VERSION_PROBE_FILE%" >nul 2>nul
    set "ERROR_HINT=Version numbers disagree between package.json, src-tauri/Cargo.toml and src-tauri/tauri.conf.json."
    exit /b 1
)
for /f "usebackq delims=" %%V in ("%VERSION_PROBE_FILE%") do set "VERSION=%%V"
del /q "%VERSION_PROBE_FILE%" >nul 2>nul
if not defined VERSION (
    set "ERROR_HINT=Could not read the version from package.json."
    exit /b 1
)
set "VERSION_OLD=%VERSION%"
echo [OK] package.json, Cargo.toml and tauri.conf.json all report %VERSION%.
echo.
exit /b 0

:validate_project
echo [3/12] Validating project files...
for %%F in ("%PROJECT_ROOT%\package.json" "%PROJECT_ROOT%\package-lock.json" "%PROJECT_ROOT%\src-tauri\Cargo.toml" "%PROJECT_ROOT%\src-tauri\Cargo.lock" "%PROJECT_ROOT%\src-tauri\tauri.conf.json" "%PROJECT_ROOT%\scripts\check-version.mjs" "%PROJECT_ROOT%\scripts\bump-version.mjs" "%PROJECT_ROOT%\scripts\write-release-docs.mjs") do (
    if not exist "%%~F" (
        set "ERROR_HINT=Required project file was not found: %%~F"
        exit /b 1
    )
)
echo [OK] Project files are complete.
echo.
exit /b 0

:prepare_log
echo [4/12] Preparing the build log...
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

rem Use the repository's Cargo cache when it is present, unless caller chose another one.
rem These have to be set before :run_tests, not just before the build - cargo test
rem uses the same target directory.
if not defined CARGO_HOME if exist "%PROJECT_ROOT%\.cargo-home" set "CARGO_HOME=%PROJECT_ROOT%\.cargo-home"
set "CARGO_TARGET_DIR=%TARGET_DIR%"
echo [OK] Build log is ready.
echo.
exit /b 0

:prepare_release_dir
echo [8/12] Preparing output directories for %VERSION%...
set "RELEASE_DIR=%PROJECT_ROOT%\release\ProviderDeck-%VERSION%-windows-x64"

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
echo [OK] Output directories are ready.
echo.
exit /b 0

:install_dependencies
echo [5/12] Installing locked JavaScript dependencies...
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
echo [6/12] Running Rust, TypeScript and frontend tests...
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

:bump_version
echo [7/12] Bumping the patch version for this release...
if defined PROVIDER_DECK_SKIP_BUMP (
    echo   - PROVIDER_DECK_SKIP_BUMP is set, keeping version %VERSION%.
    echo [OK] Version bump skipped on request.
    echo.
    exit /b 0
)

rem bump-version.mjs writes the new number to a file for this script to read.
rem Its own stdout is deliberately NOT redirected: the "old version / new version"
rem line is Chinese, and Node emits raw UTF-8 into a redirect, which the 936 code
rem page renders as mojibake in the log. Straight to the console it goes through
rem WriteConsoleW and displays correctly.
node "%PROJECT_ROOT%\scripts\bump-version.mjs" --out "%BUMP_OUT_FILE%"
if errorlevel 1 (
    del /q "%BUMP_OUT_FILE%" >nul 2>nul
    set "ERROR_HINT=Version bump failed. Nothing was packaged and no file was changed. Fix the version numbers, then run build_all.bat again."
    exit /b 1
)
if not exist "%BUMP_OUT_FILE%" (
    set "ERROR_HINT=Version bump reported success but wrote no version file: %BUMP_OUT_FILE%"
    exit /b 1
)
set "VERSION="
for /f "usebackq delims=" %%V in ("%BUMP_OUT_FILE%") do set "VERSION=%%V"
del /q "%BUMP_OUT_FILE%" >nul 2>nul
if not defined VERSION (
    set "ERROR_HINT=Could not read the bumped version number."
    exit /b 1
)
if "%VERSION%"=="%VERSION_OLD%" (
    set "ERROR_HINT=Version did not change (still %VERSION%). Refusing to publish a second release under one version number."
    exit /b 1
)

rem Re-run the independent cross-check. bump-version.mjs verifies its own writes,
rem but a script confirming its own output is one witness, not two - and this one
rem already guards the three files the installer and the EXE properties read.
node "%PROJECT_ROOT%\scripts\check-version.mjs" > "%RECHECK_FILE%" 2>&1
if errorlevel 1 (
    echo.
    type "%RECHECK_FILE%"
    del /q "%RECHECK_FILE%" >nul 2>nul
    set "ERROR_HINT=Version numbers disagree after the bump. The working tree now holds a partial bump - inspect package.json, src-tauri/Cargo.toml and src-tauri/tauri.conf.json before packaging again."
    exit /b 1
)
del /q "%RECHECK_FILE%" >nul 2>nul
if not exist "%CHANGELOG_SOURCE%" (
    set "ERROR_HINT=CHANGELOG.md was not created by the version bump: %CHANGELOG_SOURCE%"
    exit /b 1
)
set "BUMPED=1"
echo Version bumped: %VERSION_OLD% -^> %VERSION% >> "%BUILD_LOG%" 2>nul
echo [OK] Version is now %VERSION% and all version files agree.
echo.
exit /b 0

:build_release
echo [9/12] Building frontend, Rust application and NSIS installer...

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
echo [10/12] Collecting installer and portable executable...
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
echo [11/12] Writing README.txt, CHANGELOG.md and release-summary.txt...
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

rem Ship the changelog alongside the binaries. Copied here rather than in
rem :write_checksums so SHA256SUMS.txt, which runs last over every file in the
rem directory, covers it too.
if exist "%CHANGELOG_SOURCE%" (
    copy /y "%CHANGELOG_SOURCE%" "%RELEASE_DIR%\CHANGELOG.md" >nul
    if errorlevel 1 (
        set "ERROR_HINT=Could not copy CHANGELOG.md into the release directory."
        exit /b 1
    )
) else (
    rem Only reachable with PROVIDER_DECK_SKIP_BUMP on a tree that never bumped.
    echo   - Note: CHANGELOG.md not found at the project root, release will ship without it.
)
echo [OK] Release documents were written.
echo.
exit /b 0

:write_checksums
rem Hash the files that actually ship, after every other file is already in
rem place, so the digests always describe this run's output.
echo [12/12] Generating SHA-256 checksums...
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
if defined BUMPED (
    rem Do not claim the tree is untouched once the bump has landed. The version
    rem is already at %VERSION% on disk, and saying otherwise sends the operator
    rem looking for a problem in the wrong place.
    echo NOTE: the version was already bumped to %VERSION% before this failure.
    echo Either fix the cause and set PROVIDER_DECK_SKIP_BUMP=1 to rebuild %VERSION%,
    echo or revert the version change in package.json, src-tauri\Cargo.toml,
    echo src-tauri\tauri.conf.json, src-tauri\Cargo.lock and CHANGELOG.md.
) else (
    echo Source files and previous versioned release folders were not changed.
)
exit /b 1
