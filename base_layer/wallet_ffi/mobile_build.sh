#!/bin/bash
#
# Script to build libraries for Tari Wallet
#

#Terminal colors
RED='\033[0;31m'
GREEN='\033[0;32m'
PURPLE='\033[0;35m'
CYAN='\033[0;36m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

source build.config
CURRENT_DIR=${TARI_REPO_PATH}/base_layer/wallet_ffi
cd ${CURRENT_DIR} || exit
timestamp=$(date +%s)
mkdir -p logs
cd logs || exit
mkdir -p ${timestamp}
cd ${timestamp} || exit
mkdir -p ios
mkdir -p android
cd ../..
IOS_LOG_PATH=${CURRENT_DIR}/logs/${timestamp}/ios
ANDROID_LOG_PATH=${CURRENT_DIR}/logs/${timestamp}/android
ZMQ_FOLDER=libzmq
SQLITE_FOLDER=sqlite
cd ../../..

unameOut="$(uname -s)"
case "${unameOut}" in
    Linux*)     MACHINE=Linux;;
    Darwin*)    MACHINE=Mac;;
    CYGWIN*)    MACHINE=Cygwin;;
    MINGW*)     MACHINE=MinGw;;
    *)          MACHINE="UNKNOWN:${unameOut}"
esac
export PKG_CONFIG_ALLOW_CROSS=1

# Fix for macOS Catalina failing to include correct headers for cross compilation
if [ "${MACHINE}" == "Mac" ]; then
  MAC_VERSION=$(sw_vers -productVersion)
  MAC_SUB_VERSION=$(cut -d '.' -f2 <<<"${MAC_VERSION}")
  if [ "${MAC_SUB_VERSION}" -ge 15 ]; then
    unset CPATH
    echo "${PURPLE}macOS 15+ Detected${NC}"
  else
     echo "${PURPLE}macOS 14 Detected${NC}"
  fi
fi

DEPENDENCIES=${IOS_WALLET_PATH}
# PKG_PATH, BUILD_IOS is defined in build.config
# shellcheck disable=SC2153
if [ -n "${DEPENDENCIES}" ] && [ -n "${PKG_PATH}" ] && [ "${BUILD_IOS}" -eq 1 ] && [ "${MACHINE}" == "Mac" ]; then
  echo "${GREEN}Commencing iOS build${NC}"
  echo "${YELLOW}Build logs can be found at ${IOS_LOG_PATH}${NC}"
  echo "\t${CYAN}Configuring Rust${NC}"
  rustup target add aarch64-apple-ios x86_64-apple-ios >> ${IOS_LOG_PATH}/rust.txt 2>&1
  cargo install cargo-lipo >> ${IOS_LOG_PATH}/rust.txt 2>&1
  echo "\t${CYAN}Configuring complete${NC}"
  ZMQ_BUILD_FOUND=0
  if [ -f ${DEPENDENCIES}/MobileWallet/TariLib/libzmq.a ]; then
    ZMQ_BUILD_FOUND=1
  fi
  #below line is temporary
  ZMQ_REPO_IOS="https://github.com/azawawi/libzmq-ios"
  cd ${DEPENDENCIES} || exit
  mkdir -p build
  cd build || exit
  BUILD_ROOT=$PWD
  cd ..
  if [ ${ZMQ_BUILD_FOUND} -eq 0 ]; then
    echo "\t${CYAN}Fetching ZMQ source${NC}"
    if [ ! -d "${ZMQ_FOLDER}-ios" ]; then
      git clone ${ZMQ_REPO_IOS} > ${IOS_LOG_PATH}/zmq.txt 2>&1
      cd ${ZMQ_FOLDER}-ios || exit
    else
      cd ${ZMQ_FOLDER}-ios || exit
      git pull > ${IOS_LOG_PATH}/zmq.txt 2>&1
    fi
    echo "\t${CYAN}Source fetched${NC}"
    # On macOS catalina, build for 32bit will throw linker error for unsupported architecture, can be safely ignored.
    # Only libs we interested in from the below build script are for aarch64 and x86_64
    echo "\t${CYAN}Building ZMQ${NC}"
    ruby libzmq.rb >> ${IOS_LOG_PATH}/zmq.txt 2>&1
    echo "\t${CYAN}ZMQ built${NC}"
    cp "${PWD}/dist/ios/lib/libzmq.a" "${DEPENDENCIES}/MobileWallet/TariLib/"
  else
    echo "\t${CYAN}ZMQ located${NC}"
  fi
  cd ${CURRENT_DIR} || exit
  cargo clean
  cp wallet.h "${DEPENDENCIES}/MobileWallet/TariLib/"
  export PKG_CONFIG_PATH=${PKG_PATH}
  echo "\t${CYAN}Building Wallet FFI${NC}"
  cargo-lipo lipo --release > ${IOS_LOG_PATH}/cargo.txt 2>&1
  cd ../..
  cd target || exit
  cd universal || exit
  cd release || exit
  cp libwallet_ffi.a "${DEPENDENCIES}/MobileWallet/TariLib/"
  cd ../../.. || exit
  rm -rf target
  cd ${DEPENDENCIES} || exit
  rm -rf ${ZMQ_FOLDER}-ios
  echo "${GREEN}iOS build completed${NC}"
else
  if [ "${BUILD_IOS}" -eq 1 ]; then
    echo "${RED}Cannot configure iOS Wallet Library build${NC}"
  else
    echo "${GREEN}iOS Wallet is configured not to build${NC}"
  fi
fi

DEPENDENCIES=$ANDROID_WALLET_PATH
# PKG_PATH, BUILD_ANDROID, NDK_PATH is defined in build.config
# shellcheck disable=SC2153
if [ -n "${DEPENDENCIES}" ] && [ -n "${NDK_PATH}" ] && [ -n "${PKG_PATH}" ] && [ "${BUILD_ANDROID}" -eq 1 ]; then
  echo "${GREEN}Commencing Android build${NC}"
  echo "${YELLOW}Build logs can be found at ${IOS_LOG_PATH}${NC}"
  echo "\t${CYAN}Configuring Rust${NC}"
  rustup target add x86_64-linux-android aarch64-linux-android armv7-linux-androideabi i686-linux-android arm-linux-androideabi > ${ANDROID_LOG_PATH}/rust.txt 2>&1
  if [ "${MAC_SUB_VERSION}" -lt 15 ]; then
    cargo install cargo-ndk >> ${ANDROID_LOG_PATH}/rust.txt 2>&1
  fi
  echo "\t${CYAN}Configuring complete${NC}"
  export NDK_HOME=${NDK_PATH}
  export PKG_CONFIG_PATH=${PKG_PATH}
  export NDK_TOOLCHAIN_VERSION=clang
  DEPENDENCIES=${DEPENDENCIES}/jniLibs

  ZMQ_BUILD_FOUND=0
  if [ -f ${DEPENDENCIES}/i686/libzmq.a ] && [ -f ${DEPENDENCIES}/x86_64/libzmq.a ] && [ -f ${DEPENDENCIES}/armeabi-v7a/libzmq.a ] && [ -f ${DEPENDENCIES}/arm64-v8a/libzmq.a ]; then
    ZMQ_BUILD_FOUND=1
  fi

  SQLITE_BUILD_FOUND=0
  if [ -f ${DEPENDENCIES}/i686/libsqlite3.a ] && [ -f ${DEPENDENCIES}/x86_64/libsqlite3.a ] && [ -f ${DEPENDENCIES}/armeabi-v7a/libsqlite3.a ] && [ -f ${DEPENDENCIES}/arm64-v8a/libsqlite3.a ]; then
    SQLITE_BUILD_FOUND=1
  fi

  cd ${DEPENDENCIES} || exit
  mkdir -p build
  cd build || exit
  BUILD_ROOT=${PWD}
  if [ "${MACHINE}" == "Mac" ]; then
    if [ "${MAC_SUB_VERSION}" -ge 15 ]; then
      cd ${NDK_HOME}/sources/cxx-stl/llvm-libc++/include || exit
      mkdir -p sys
      #Fix for missing header, c code should reference limits.h instead of syslimits.h, happens with code that has been around for a long time.
      cp "${NDK_HOME}/sources/cxx-stl/llvm-libc++/include/limits.h" "${NDK_HOME}/sources/cxx-stl/llvm-libc++/include/sys/syslimits.h"
      cd ${BUILD_ROOT} || exit
    fi
  fi
  cd ..
  if [ ${ZMQ_BUILD_FOUND} -eq 0 ]; then
    echo "\t${CYAN}Fetching ZMQ source${NC}"
    if [ ! -d ${ZMQ_FOLDER} ]; then
      git clone ${ZMQ_REPO} > ${ANDROID_LOG_PATH}/zmq.txt 2>&1
      cd ${ZMQ_FOLDER} || exit
    else
      cd ${ZMQ_FOLDER} || exit
      git pull > ${ANDROID_LOG_PATH}/zmq.txt 2>&1
    fi
    echo "\t${CYAN}Source fetched${NC}"
  else
    echo "\t${CYAN}ZMQ located${NC}"
  fi
  for PLATFORMABI in "aarch64-linux-android" "x86_64-linux-android" "i686-linux-android" "armv7-linux-androideabi"
  do
    # Lint warning for loop only running once is acceptable here
    # shellcheck disable=SC2043
    for LEVEL in 24
    #21 22 23 26 26 27 28 29 not included at present
    do
      PLATFORM=$(cut -d'-' -f1 <<<"${PLATFORMABI}")

      PLATFORM_OUTDIR=""
      if [ "${PLATFORM}" == "i686" ]; then
        PLATFORM_OUTDIR="x86"
        elif [ "${PLATFORM}" == "x86_64" ]; then
          PLATFORM_OUTDIR="x86_64"
        elif [ "${PLATFORM}" == "armv7" ]; then
          PLATFORM_OUTDIR="armeabi-v7a"
        elif [ "${PLATFORM}" == "aarch64" ]; then
          PLATFORM_OUTDIR="arm64-v8a"
        else
          PLATFORM_OUTDIR=${PLATFORM}
      fi
      cd ${BUILD_ROOT} || exit
      mkdir -p ${PLATFORM_OUTDIR}
      OUTPUT_DIR=${BUILD_ROOT}/${PLATFORM_OUTDIR}
      cd ${DEPENDENCIES} || exit

      PLATFORMABI_TOOLCHAIN=${PLATFORMABI}
      PLATFORMABI_COMPILER=${PLATFORMABI}
      if [ "${PLATFORMABI}" == "armv7-linux-androideabi" ]; then
        PLATFORMABI_TOOLCHAIN="arm-linux-androideabi"
        PLATFORMABI_COMPILER="armv7a-linux-androideabi"
      fi
      # set toolchain path
      export TOOLCHAIN=${NDK_HOME}/toolchains/llvm/prebuilt/darwin-x86_64/${PLATFORMABI_TOOLCHAIN}

      # set the archiver
      export AR=${NDK_HOME}/toolchains/llvm/prebuilt/darwin-x86_64/bin/${PLATFORMABI_TOOLCHAIN}$'-'ar

      # set the assembler
      export AS=${NDK_HOME}/toolchains/llvm/prebuilt/darwin-x86_64/bin/${PLATFORMABI_TOOLCHAIN}$'-'as

      # set the c and c++ compiler
      CC=${NDK_HOME}/toolchains/llvm/prebuilt/darwin-x86_64/bin/${PLATFORMABI_COMPILER}
      export CC=${CC}${LEVEL}$'-'clang
      export CXX=${CC}++

      export CXXFLAGS="-stdlib=libstdc++ -isystem ${NDK_HOME}/sources/cxx-stl/llvm-libc++/include"
      # set the linker
      export LD=${NDK_HOME}/toolchains/llvm/prebuilt/darwin-x86_64/bin/${PLATFORMABI_TOOLCHAIN}$'-'ld

      # set linker flags
      export LDFLAGS="-L${NDK_HOME}/toolchains/llvm/prebuilt/darwin-x86_64/sysroot/usr/lib/${PLATFORMABI_TOOLCHAIN}/${LEVEL} -L${OUTPUT_DIR}/lib -lc++"

      # set the archive index generator tool
      export RANLIB=${NDK_HOME}/toolchains/llvm/prebuilt/darwin-x86_64/bin/${PLATFORMABI_TOOLCHAIN}$'-'ranlib

      # set the symbol stripping tool
      export STRIP=${NDK_HOME}/toolchains/llvm/prebuilt/darwin-x86_64/bin/${PLATFORMABI_TOOLCHAIN}$'-'strip

      # set c flags
      #note: Add -v to below to see compiler output, include paths, etc
      export CFLAGS="-DMDB_USE_ROBUST=0"

      # set cpp flags
      export CPPFLAGS="-fPIC -I${OUTPUT_DIR}/include"

      mkdir -p ${SQLITE_FOLDER}
      cd ${SQLITE_FOLDER} || exit
      if [ ${SQLITE_BUILD_FOUND} -eq 0 ]; then
        echo "\t${CYAN}Fetching Sqlite3 source${NC}"
        curl -s ${SQLITE_SOURCE} | tar -xvf - -C ${PWD} > ${ANDROID_LOG_PATH}/sqlite.txt 2>&1
        echo "\t${CYAN}Source fetched${NC}"
        cd * || exit
        echo "\t${CYAN}Building Sqlite3${NC}"
        make clean >> ${ANDROID_LOG_PATH}/sqlite.txt 2>&1
        ./configure --host=${PLATFORMABI} --prefix=${OUTPUT_DIR} >> ${ANDROID_LOG_PATH}/sqlite.txt 2>&1
        make install >> ${ANDROID_LOG_PATH}/sqlite.txt 2>&1
        echo "\t${CYAN}Sqlite3 built${NC}"
      else
        echo "\t${CYAN}Sqlite3 located${NC}"
      fi
      cd ../..

      cd ${ZMQ_FOLDER} || exit
      if [ ${ZMQ_BUILD_FOUND} -eq 0 ]; then
        echo "\t${CYAN}Building ZMQ${NC}"
        make clean >> ${ANDROID_LOG_PATH}/zmq.txt 2>&1
        ./autogen.sh >> ${ANDROID_LOG_PATH}/zmq.txt 2>&1
        ./configure --enable-static --disable-shared --host=${PLATFORMABI} --prefix=${OUTPUT_DIR} --without-docs >> ${ANDROID_LOG_PATH}/zmq.txt >> ${ANDROID_LOG_PATH}/zmq.txt 2>&1
        make install >> ${ANDROID_LOG_PATH}/zmq.txt 2>&1
        echo "\t${CYAN}ZMQ built${NC}"
      fi
      if [ "${MACHINE}" == "Mac" ]; then
        if [ "${MAC_SUB_VERSION}" -ge 15 ]; then
          # Not ideal, however necesary for cargo to pass additional flags
          export CFLAGS="${CFLAGS} -I${NDK_HOME}/sources/cxx-stl/llvm-libc++/include -I${NDK_HOME}/toolchains/llvm/prebuilt/darwin-x86_64/sysroot/usr/include -I${NDK_HOME}/sysroot/usr/include/${PLATFORMABI}"
        fi
      fi
      export LDFLAGS="-L${NDK_HOME}/toolchains/llvm/prebuilt/darwin-x86_64/sysroot/usr/lib/${PLATFORMABI_TOOLCHAIN}/${LEVEL} -L${OUTPUT_DIR}/lib -lc++ -lzmq -lsqilte3"
      cd ${OUTPUT_DIR}/lib || exit

      if [ ${ZMQ_BUILD_FOUND} -eq 1 ]; then
        rm *.so
        rm *.la
      else
        cp ${DEPENDENCIES}/${PLATFORM_OUTDIR}/libzmq.a ${OUTPUT_DIR}/lib/libzmq.a
      fi
      if [ ${SQLITE_BUILD_FOUND} -eq 1 ]; then
       cp ${DEPENDENCIES}/${PLATFORM_OUTDIR}/libsqlite3.a ${OUTPUT_DIR}/lib/libsqlite3.a
      fi

      echo "\t${CYAN}Configuring Cargo${NC}"
      cd ${CURRENT_DIR} || exit
      cargo clean > ${ANDROID_LOG_PATH}/cargo.txt 2>&1
      mkdir -p .cargo
      cd .cargo || exit
      if [ "${MACHINE}" == "Mac" ]; then
        if [ "${MAC_SUB_VERSION}" -ge 15 ]; then
          cat > config <<EOF
[build]
target = "${PLATFORMABI}"

[target.${PLATFORMABI}]
ar = "${AR}"
linker = "${CC}"
rustflags = "-L${OUTPUT_DIR}/lib"

[target.${PLATFORMABI}.zmq]
rustc-flags = "-L${OUTPUT_DIR}/lib"
EOF

        else
          cat > config <<EOF
[target.${PLATFORMABI}]
ar = "${AR}"
linker = "${CC}"

[target.${PLATFORMABI}.zmq]
rustc-flags = "-L${OUTPUT_DIR}/lib"
EOF

        fi
      fi
      echo "\t${CYAN}Configuring complete${NC}"
      cd .. || exit
      echo "\t${CYAN}Building Wallet FFI${NC}"
      #note: add -vv to below to see verbose and build script output
      if [ "${MACHINE}" == "Mac" ]; then
        if [ "${MAC_SUB_VERSION}" -ge 15 ]; then
          cargo build --lib --release >> ${ANDROID_LOG_PATH}/cargo.txt 2>&1
        else
          cargo ndk --target ${PLATFORMABI} --android-platform ${LEVEL} -- build --release >> ${ANDROID_LOG_PATH}/cargo.txt 2>&1
        fi
      else
        cargo ndk --target ${PLATFORMABI} --android-platform ${LEVEL} -- build --release >> ${ANDROID_LOG_PATH}/cargo.txt 2>&1
      fi
      cp wallet.h ${DEPENDENCIES}/
      rm -rf .cargo
      cd ../..
      cd target || exit
      cd ${PLATFORMABI} || exit
      cd release || exit
      cp libwallet_ffi.a ${OUTPUT_DIR}
      cd ../..
      rm -rf target
      cd ${DEPENDENCIES} || exit
      mkdir -p ${PLATFORM_OUTDIR}
      cd ${PLATFORM_OUTDIR} || exit
      if [ ${SQLITE_BUILD_FOUND} -eq 0 ]; then
        cp ${OUTPUT_DIR}/lib/libsqlite3.a ${PWD}
      fi
      cp ${OUTPUT_DIR}/libwallet_ffi.a ${PWD}
      if [ ${ZMQ_BUILD_FOUND} -eq 0 ]; then
        cp ${OUTPUT_DIR}/lib/libzmq.a ${PWD}
      fi
    done
  done
  cd ${DEPENDENCIES} || exit
  rm -rf build
  rm -rf ${ZMQ_FOLDER}
  rm -rf ${SQLITE_FOLDER}
  echo "${GREEN}Android build completed${NC}"
else
  if [ "${BUILD_ANDROID}" -eq 1 ]; then
    echo "${RED}Cannot configure Android Wallet Library build${NC}"
  else
    echo "${GREEN}Android Wallet is configured not to build${NC}"
  fi
fi
