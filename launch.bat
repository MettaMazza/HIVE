@echo off
REM ══════════════════════════════════════════════════════════════════
REM  🐝 HIVE — One-Click Launcher (Windows)
REM ══════════════════════════════════════════════════════════════════
REM
REM  This script does EVERYTHING:
REM    1. Checks if Docker Desktop is installed
REM    2. Starts Docker if it's not running
REM    3. Builds the HIVE container
REM    4. Launches HIVE with all mesh services
REM    5. Opens HivePortal in your browser
REM
REM  Usage: Double-click launch.bat or run from command prompt
REM
REM ══════════════════════════════════════════════════════════════════

setlocal enabledelayedexpansion
title HIVE - Mesh Network Launcher

echo.
echo ═══════════════════════════════════════════════════════
echo   🐝 HIVE — Human Internet Viable Ecosystem
echo ═══════════════════════════════════════════════════════
echo.

REM Handle stop/rebuild
if "%1"=="stop" (
    echo [HIVE] Stopping HIVE...
    docker compose down 2>nul
    echo [HIVE] ✅ HIVE stopped.
    pause
    exit /b 0
)

if "%1"=="rebuild" (
    echo [HIVE] Rebuilding HIVE from source...
    docker compose down 2>nul
    docker compose build --no-cache
    echo [HIVE] ✅ Rebuild complete. Run launch.bat to start.
    pause
    exit /b 0
)

REM Check for Docker
where docker >nul 2>nul
if %ERRORLEVEL% neq 0 (
    echo [WARN] Docker not found on this system.
    echo.
    echo [HIVE] Downloading Docker Desktop installer...
    echo.
    
    REM Download Docker Desktop installer
    powershell -Command "Invoke-WebRequest -Uri 'https://desktop.docker.com/win/main/amd64/Docker%%20Desktop%%20Installer.exe' -OutFile '%TEMP%\DockerInstaller.exe'"
    
    if exist "%TEMP%\DockerInstaller.exe" (
        echo [HIVE] Running Docker Desktop installer...
        echo [HIVE] Please follow the installation wizard.
        start /wait "" "%TEMP%\DockerInstaller.exe" install --quiet
        del "%TEMP%\DockerInstaller.exe" 2>nul
        echo.
        echo [HIVE] ✅ Docker installed!
        echo [WARN] ⏳ Please restart your computer, then run launch.bat again.
        pause
        exit /b 0
    ) else (
        echo [ERROR] Download failed. Install Docker Desktop manually:
        echo [ERROR]   https://docs.docker.com/desktop/install/windows-install/
        pause
        exit /b 1
    )
)

echo [HIVE] ✅ Docker found.

REM Check Docker is running
docker info >nul 2>nul
if %ERRORLEVEL% neq 0 (
    echo [WARN] Docker is installed but not running.
    echo [HIVE] Starting Docker Desktop...
    start "" "C:\Program Files\Docker\Docker\Docker Desktop.exe" 2>nul

    echo        Waiting for Docker to be ready...
    set /a count=0
    :wait_loop
    docker info >nul 2>nul
    if %ERRORLEVEL% equ 0 goto docker_ready
    set /a count+=1
    if !count! gtr 90 (
        echo.
        echo [ERROR] Docker didn't start. Please open Docker Desktop manually and try again.
        pause
        exit /b 1
    )
    timeout /t 1 >nul
    goto wait_loop
)

:docker_ready
echo [HIVE] ✅ Docker is running.

REM Check docker compose
docker compose version >nul 2>nul
if %ERRORLEVEL% neq 0 (
    echo [ERROR] Docker Compose not found. Please reinstall Docker Desktop.
    pause
    exit /b 1
)

echo [HIVE] ✅ Docker Compose found.
echo.
echo [HIVE] 🔨 Building HIVE container (this takes ~5 min first time)...
echo.

docker compose up -d --build

echo.
echo [HIVE] ✅ HIVE is running!
echo.
echo   Your mesh network is live:
echo.
echo   🏠 HivePortal    → http://localhost:3035  ← START HERE
echo   🌐 HiveSurface   → http://localhost:3032
echo   💬 HiveChat      → http://localhost:3034
echo   💻 Apis Code     → http://localhost:3033
echo   📖 Apis Book     → http://localhost:3031
echo   👁️  Panopticon    → http://localhost:3030
echo.
echo   Commands:
echo     launch.bat stop     — Stop HIVE
echo     launch.bat rebuild  — Rebuild after updates
echo     docker logs -f hive-mesh  — View live logs
echo.

REM Open browser
timeout /t 2 >nul
start http://localhost:3035

echo [HIVE] 🐝 Welcome to the mesh. You are the internet now.
echo.
pause
