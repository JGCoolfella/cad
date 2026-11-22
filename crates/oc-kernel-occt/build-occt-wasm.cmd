@echo off
rem Build the pinned OCCT (8.0.1) for WebAssembly (OVERHAUL_PLAN W1a): the same modules as
rem build-occt.cmd (Foundation, ModelingData, ModelingAlgorithms, DataExchange), static
rem archives for the wasm32-unknown-emscripten target, compiled by the pinned Emscripten.
rem Usage: build-occt-wasm.cmd <occt-source-dir> <build-dir> <install-dir>
rem Needs: emsdk with Emscripten %OC_EMSDK_VERSION% installed and activated (EMSDK_ROOT, default
rem C:\occt\emsdk), CMake >= 3.16 and Ninja (those bundled with Visual Studio 2022 are used).
rem
rem - Exceptions: native WebAssembly exception handling (-fwasm-exceptions), what Rust's
rem   wasm32-unknown-emscripten target uses; OCCT's own -fexceptions (Emscripten's JavaScript
rem   exceptions) is overridden, since the two models do not link together.
rem - -UOCC_CONVERT_SIGNALS: OCCT defines it on every compiler but MSVC; it makes OCC_CATCH_SIGNALS
rem   use setjmp/longjmp, which with WebAssembly exceptions makes clang (Emscripten 7.0.1) emit
rem   invalid code (V8: "br_table: label arity inconsistent", in ShapeUpgrade_ShapeDivide::Perform).
rem   WebAssembly has no signals to convert; the Windows build does not define it either.
rem - No threads (no -pthread): the browser build runs without SharedArrayBuffer (W1 design).
rem - "-include mutex": OCCT 8.0.1 uses std::lock_guard without <mutex> (NCollection_IncAllocator),
rem   which Emscripten's libc++ does not include through other headers.
rem - Position-independent code (OCCT adds -fPIC), so the archives also link into a side module.
setlocal
set SRC=%~1
set BLD=%~2
set INS=%~3
if "%INS%"=="" (echo usage: build-occt-wasm.cmd SRC BUILD INSTALL & exit /b 2)
if not defined EMSDK_ROOT set EMSDK_ROOT=C:\occt\emsdk
if not defined OC_EMSDK_VERSION set OC_EMSDK_VERSION=7.0.1

call "%EMSDK_ROOT%\emsdk_env.bat" >nul || exit /b 1
set PATH=C:\Program Files\Microsoft Visual Studio\2022\Community\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin;C:\Program Files\Microsoft Visual Studio\2022\Community\Common7\IDE\CommonExtensions\Microsoft\CMake\Ninja;%PATH%
call emcc --version | findstr /C:"%OC_EMSDK_VERSION%" >nul || (echo Emscripten %OC_EMSDK_VERSION% is not the active emsdk & exit /b 1)

cmake -S "%SRC%" -B "%BLD%" -G Ninja ^
  -DCMAKE_TOOLCHAIN_FILE="%EMSDK%/upstream/emscripten/cmake/Modules/Platform/Emscripten.cmake" ^
  -DCMAKE_BUILD_TYPE=Release ^
  -DCMAKE_CXX_FLAGS_RELEASE="-O2 -DNDEBUG -fwasm-exceptions -UOCC_CONVERT_SIGNALS -include mutex" ^
  -DCMAKE_C_FLAGS_RELEASE="-O2 -DNDEBUG -fwasm-exceptions -UOCC_CONVERT_SIGNALS" ^
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
  -DBUILD_DOC_Overview=OFF ^
  -DUSE_TK=OFF -DUSE_TCL=OFF -DUSE_FREETYPE=OFF -DUSE_FREEIMAGE=OFF ^
  -DUSE_OPENGL=OFF -DUSE_GLES2=OFF -DUSE_D3D=OFF -DUSE_VTK=OFF ^
  -DUSE_RAPIDJSON=OFF -DUSE_DRACO=OFF -DUSE_TBB=OFF || exit /b 1

cmake --build "%BLD%" --config Release || exit /b 1
cmake --install "%BLD%" --config Release || exit /b 1
