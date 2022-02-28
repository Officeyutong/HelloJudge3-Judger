FROM ubuntu

# RUN sed -i 's/archive.ubuntu.com/mirrors.ustc.edu.cn/g' /etc/apt/sources.list
# RUN sed -i 's/security.ubuntu.com/mirrors.ustc.edu.cn/g' /etc/apt/sources.list
RUN apt update
RUN apt install g++ -y
RUN apt install python2 -y
RUN apt install python3 -y
RUN apt install openjdk-8-jdk -y
RUN apt install haskell-platform -y
RUN apt install curl build-essential m4 zlib1g-dev libssl-dev ocaml ocaml-native-compilers opam -y
RUN apt install rustc -y
RUN apt install -y fpc
COPY testlib.h /usr/include
