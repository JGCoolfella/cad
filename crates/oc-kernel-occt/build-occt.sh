#!/usr/bin/env bash
# Build the pinned OCCT (8.0.1) that oc-kernel-occt links, on macOS or Linux. Same module
# selection as build-occt.cmd.
# Usage: build-occt.sh <occt-source-dir> <build-dir> <install-dir> [Static|Shared]
# Static (default) is what tests and CI link; Shared is the shipping configuration
# (docs/THIRD_PARTY.md, LGPL 2.1). Needs: a C++17 compiler, CMake >= 3.16, Ninja.
set -euo pipefail
SRC="$1"; BLD="$2"; INS="$3"; TYPE="${4:-Static}"

cmake -S "$SRC" -B "$BLD" -G Ninja \
  -DCMAKE_BUILD_TYPE=Release \
  -DINSTALL_DIR="$INS" \
  -DBUILD_LIBRARY_TYPE="$TYPE" \
  -DBUILD_CPP_STANDARD=C++17 \
  -DBUILD_MODULE_FoundationClasses=ON \
  -DBUILD_MODULE_ModelingData=ON \
  -DBUILD_MODULE_ModelingAlgorithms=ON \
  -DBUILD_MODULE_DataExchange=ON \
  -DBUILD_MODULE_ApplicationFramework=OFF \
  -DBUILD_MODULE_Visualization=OFF \
  -DBUILD_MODULE_Draw=OFF \
  -DBUILD_MODULE_DETools=OFF \
  -DBUILD_DOC_Overview=OFF \
  -DUSE_TK=OFF -DUSE_TCL=OFF -DUSE_FREETYPE=OFF -DUSE_FREEIMAGE=OFF \
  -DUSE_OPENGL=OFF -DUSE_GLES2=OFF -DUSE_VTK=OFF -DUSE_XLIB=OFF \
  -DUSE_RAPIDJSON=OFF -DUSE_DRACO=OFF -DUSE_TBB=OFF \
  ${OCCT_EXTRA_CMAKE_ARGS:-}

cmake --build "$BLD" --config Release
cmake --install "$BLD" --config Release
