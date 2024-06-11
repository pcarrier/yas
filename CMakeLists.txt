cmake_minimum_required(VERSION 3.29)
project(zat LANGUAGES Swift C)

add_subdirectory(quickjs)

add_executable(zat zat.swift)
