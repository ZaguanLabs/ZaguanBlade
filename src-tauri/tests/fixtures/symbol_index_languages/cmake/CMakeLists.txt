cmake_minimum_required(VERSION 3.22)
project(ZBladeFixture)

include(GNUInstallDirs)
set(ZBLADE_FEATURES ON)
add_library(zblade_core src/core.cpp)
add_executable(zblade_cli src/main.cpp)

function(configure_zblade target)
endfunction()

macro(register_zblade_test name)
endmacro()
