ARG CROSS_BASE_IMAGE
FROM $CROSS_BASE_IMAGE

# requirements of bindgen, see https://rust-lang.github.io/rust-bindgen/requirements.html
RUN DEBIAN_FRONTEND=noninteractive apt-get update && \
    apt-get install -y libopencv-dev llvm-dev libclang-dev clang

# cross compile opencv, see https://docs.opencv.org/4.x/d0/d76/tutorial_arm_crosscompile_with_cmake.html
#RUN DEBIAN_FRONTEND=noninteractive apt-get install -y gcc-arm-linux-gnueabihf git build-essential cmake
#RUN git clone --depth 1 --branch '4.x' https://github.com/opencv/opencv.git && \
#    cd opencv/platforms/linux && \
#    mkdir build && \
#    cd build && \
#    cmake -DCMAKE_TOOLCHAIN_FILE=../arm-gnueabi.toolchain.cmake ../../.. && \
#    make && \
#    make install

ENV CMAKE_PREFIX_PATH="/opencv/platforms/linux/build/install"