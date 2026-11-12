@echo off
rem P1 spike: build a minimal static OCCT (no visualisation, no Tcl/Tk, no FreeType).
rem Usage: build-occt.cmd <occt-source-dir> <build-dir> <install-dir>
rem Needs: Visual Studio 2022 C++ tools, CMake >= 3.16, Ninja.
setlocal
set SRC=%~1
set BLD=%~2
set INS=%~3
if "%INS%"=="" (echo usage: build-occt.cmd SRC BUILD INSTALL & exit /b 2)

if not defined VCINSTALLDIR call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat" >nul || exit /b 1
set PATH=C:\Program Files\Microsoft Visual Studio\2022\Community\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin;%PATH%

cmake -S "%SRC%" -B "%BLD%" -G Ninja ^
  -DCMAKE_BUILD_TYPE=Release ^
  -DINSTALL_DIR="%INS%" ^
  -DBUILD_LIBRARY_TYPE=Static ^
  -DBUILD_CPP_STANDARD=C++17 ^
  -DBUILD_MODULE_FoundationClasses=ON ^
  -DBUILD_MODULE_ModelingData=ON ^
  -DBUILD_MODULE_ModelingAlgorithms=ON ^
  -DBUILD_MODULE_DataExchange=ON ^
  -DBUILD_MODULE_ApplicationFramework=OFF ^
  -DBUILD_MODULE_Visualization=OFF ^
  -DBUILD_MODULE_Draw=OFF ^
  -DBUILD_MODULE_DETools=OFF ^
  -DBUILD_DOC_Overview=OFF ^
  -DUSE_TK=OFF -DUSE_TCL=OFF -DUSE_FREETYPE=OFF -DUSE_FREEIMAGE=OFF ^
  -DUSE_OPENGL=OFF -DUSE_GLES2=OFF -DUSE_D3D=OFF -DUSE_VTK=OFF ^
  -DUSE_RAPIDJSON=OFF -DUSE_DRACO=OFF -DUSE_TBB=OFF || exit /b 1

cmake --build "%BLD%" --config Release || exit /b 1
cmake --install "%BLD%" --config Release || exit /b 1
