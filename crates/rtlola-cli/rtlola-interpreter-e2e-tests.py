#!/usr/bin/env python3

import json
import time
from pathlib import Path
from junit_xml import TestSuite, TestCase
import subprocess
import platform
import sys
import argparse
import re
import os

EXIT_FAILURE = 1

def build_path(base_dir, parts):
    path = base_dir
    for part in parts:
        path = path.joinpath(part)
    return path


def print_fail(message, end='\n'):
    sys.stdout.write('\x1b[1;31m' + message.rstrip() + '\x1b[0m' + end)


def print_pass(message, end='\n'):
    sys.stdout.write('\x1b[1;32m' + message.rstrip() + '\x1b[0m' + end)


def print_warn(message, end='\n'):
    sys.stdout.write('\x1b[1;33m' + message.rstrip() + '\x1b[0m' + end)


def print_info(message, end='\n'):
    sys.stdout.write('\x1b[1;34m' + message.rstrip() + '\x1b[0m' + end)


def print_bold(message, end='\n'):
    sys.stdout.write('\x1b[1;37m' + message.rstrip() + '\x1b[0m' + end)


def print_additional_trigger(message, count):
    sys.stdout.write('\x1b[1;31m"' + message.rstrip() + "\" : " + str(count) + ' (0 expected) \x1b[0m\n')


def print_trigger(message, expected, actual):
    if actual < expected:
        print_trigger_not_enough(trigger, expected, actual)
    else:
        print_trigger_too_many(trigger, expected, actual)


def print_trigger_not_enough(message, expected, actual):
    sys.stdout.write('"' + message.strip() + '\"\x1b[1;34m' + " : {} ({} expected)".format(actual, expected) + '\x1b[0m\n')


def print_trigger_too_many(message, expected, actual):
    sys.stdout.write('"' + message.strip() + '\"\x1b[1;31m' + " : {} ({} expected)".format(actual, expected) + '\x1b[0m\n')


def run_offline():
    res = subprocess.run([rtlola_interpreter_executable_path_string, "monitor", "--offline", "relative-secs", "--stdout", "--verbosity", "trigger", str(spec_file), "--csv-in", str(input_file)] + config, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, cwd=str(repo_base_dir), universal_newlines=True, timeout=10)
    return res.returncode, iter(res.stdout.split("\n"))

def run_online():
    with open(str(input_file), "r") as csv:
        input_lines = [line.strip() for line in csv.readlines()]
    time_idx = input_lines[0].split(',').index("time")

    out_file = open("temp_test_output.txt", "w+")
    monitor = subprocess.Popen([rtlola_interpreter_executable_path_string, "monitor", "--online", "--stdout", "--verbosity", "trigger", "--stdin", str(spec_file)] + config, stdout=out_file, stderr=subprocess.STDOUT, cwd=str(repo_base_dir), stdin=subprocess.PIPE, universal_newlines=True)

    # write csv header
    monitor.stdin.write(input_lines[0]+os.linesep)

    # write first event
    last_event_time = float(input_lines[1].split(',')[time_idx])
    monitor.stdin.write(input_lines[1]+os.linesep)
    monitor.stdin.flush()

    for line in input_lines[2:]:
        cur_time = float(line.split(',')[time_idx])
        due_time = cur_time - last_event_time
        last_event_time = cur_time
        time.sleep(due_time)
        monitor.stdin.write(line+os.linesep)
        monitor.stdin.flush()

    monitor.stdin.close()
    monitor.wait(timeout=10)
    out_file.close()
    with open("temp_test_output.txt", "r") as f:
        lines = f.readlines()
    out_file.close()
    os.remove("temp_test_output.txt")
    return monitor.returncode, iter(lines)

parser = argparse.ArgumentParser(description='Run end-to-end tests for rtlola-interpreter')
parser.add_argument("--online", action='store_true', help="Additionally runs all tests in online mode.", dest="online")
args = parser.parse_args()
if args.online:
    run_mode = "online"
else:
    run_mode = "offline"

running_on_windows = platform.system() == "Windows"
executable_name = "rtlola-cli.exe" if running_on_windows else "rtlola-cli"

build_mode = os.getenv("BUILD_MODE", default="debug")

repo_base_dir = Path(os.getcwd())
if not Path(".gitlab-ci.yml").exists():
    if (repo_base_dir.parent/".gitlab-ci.yml").exists():
        repo_base_dir = repo_base_dir.parent
    else:
        print_fail("Run this script from the repo base or from the crates directory!")
        sys.exit(EXIT_FAILURE)
repo_base_dir = repo_base_dir/"crates"

rtlola_interpreter_executable_path = repo_base_dir / "target" / build_mode / executable_name
rtlola_interpreter_executable_path_string = str(rtlola_interpreter_executable_path)

if build_mode == "debug":
    cargo_build = subprocess.run(["cargo", "build", "--bin", "rtlola-cli", "--all-features"], cwd=str(repo_base_dir))
elif build_mode == "release":
    cargo_build = subprocess.run(["cargo", "build", "--bin", "rtlola-cli", "--all-features", "--release"], cwd=str(repo_base_dir))
else:
    print("invalid BUILD_MODE '{}'".format(build_mode))
    sys.exit(EXIT_FAILURE)
if cargo_build.returncode != 0:
    sys.exit(EXIT_FAILURE)

total_number_of_tests = 0
crashed_tests = 0
wrong_tests = 0
tests_passed = 0

test_dir = repo_base_dir/"tests/definitions"
tests = [test_file for test_file in test_dir.iterdir() if test_file.is_file() and test_file.suffix == ".rtlola_interpreter_test"]

tests_passed = []
tests_crashed = []
tests_wrong_out = []
return_code = 0

ansi_escape = re.compile(r'\x1B[@-_][0-?]*[ -/]*[@-~]')

with open(repo_base_dir/"tests/e2e-results.xml", 'w') as results_file:
    testcases = []
    for (mode, config) in [('closure', []), ('time-info', ["--output-time-format", "relative-secs"])]:
        check_time_info = "--output-time-format" in config
        for test_file in tests:
            with test_file.open() as fd:
                test_json = json.load(fd)
                spec_file = build_path(repo_base_dir, ["tests"]+test_json["spec_file"].split('/')[1:])
                input_file = build_path(repo_base_dir, ["tests"]+test_json["input_file"].split('/')[1:])
                is_pcap = len(test_json["modes"]) > 0 and test_json["modes"][0] == "pcap"

                if not (is_pcap or run_mode in test_json["modes"]):
                    continue

                total_number_of_tests += 1
                print("========================================================================")
                test_name = "{} @ {}".format(mode, test_file.name.split('.')[0])
                print_bold("{}:".format(test_name))
                timed_out = False
                err_out = []

                something_wrong = False
                returncode = None
                try:
                    if is_pcap:
                        run_result = subprocess.run([rtlola_interpreter_executable_path_string, "ids", "--stdout", "--verbosity", "trigger", str(spec_file), "192.168.178.0/24", "--pcap-in", str(input_file)] + config, stdout=subprocess.PIPE, stderr=subprocess.PIPE, cwd=str(repo_base_dir), universal_newlines=True, timeout=10)
                        returncode = run_result.returncode
                        lines = iter(run_result.stdout.split("\n"))
                    elif run_mode == "offline":
                        (returncode, lines) = run_offline()
                    elif run_mode == "online":
                        (returncode, lines) = run_online()

                except subprocess.TimeoutExpired:
                    tests_crashed.append(test_name)
                    print_fail("Test timed out")
                    something_wrong = True
                    timed_out = True

                if returncode is not None:
                    if returncode == 0:
                        triggers_in_output = dict()

                        # count triggers
                        for line in lines:
                            if line == "":
                                continue
                            line = ansi_escape.sub(r'', line)
                            m = re.match(r'\[(?P<timeinfo>\d+\.\d+)\]\[Trigger\]\[#\d+\]\s(?P<trig_msg>.*)\r?\n?$', line)
                            if m:
                                timeinfo = m.group('timeinfo')
                                trig_msg = m.group('trig_msg')
                                triggers_in_output.setdefault(trig_msg, [])
                                triggers_in_output[trig_msg].append(timeinfo)
                                continue
                            #print("Unexpected line: {}".format(line))

                        # print diff in triggers
                        # TODO allow for specifying a tolerance in the JSON
                        expected_triggers = list(test_json["triggers"].keys())
                        trigger_names = list(set(list(triggers_in_output.keys()) + expected_triggers))
                        trigger_names.sort()
                        for trigger in trigger_names:
                            if trigger in expected_triggers:
                                actual_time_info = triggers_in_output[trigger] if trigger in triggers_in_output else []
                                actual_count = len(actual_time_info)
                                expected_time_info = test_json["triggers"][trigger]["time_info"]
                                expected_count = test_json["triggers"][trigger]["expected_count"]
                                if expected_count != len(expected_time_info):
                                    print_fail("trigger \"{}\": 'time_info' does not match 'expected_count'".format(trigger))
                                    err_out.append("trigger \"{}\": 'time_info' does not match 'expected_count'".format(trigger))
                                    something_wrong = True
                                elif actual_count != expected_count:
                                    print_trigger(trigger, expected_count, actual_count)
                                    err_out.append("trigger \"{}\":  : {} ({} expected)".format(trigger,actual_count, expected_count))
                                    something_wrong = True
                                elif run_mode == "offline" and check_time_info and actual_time_info != expected_time_info:
                                    # only check time in offline mode
                                    print_fail("time info for trigger \"{}\" incorrect:".format(trigger))
                                    err_out.append("time info for trigger \"{}\" incorrect:".format(trigger))
                                    print_info("got | wanted")
                                    err_out.append("got | wanted")
                                    for (actual, expected) in zip(actual_time_info, expected_time_info):
                                        row = "{} | {}".format(actual, expected)
                                        err_out.append("{} | {}".format(actual, expected))
                                        if actual != expected:
                                            print_fail(row)
                                        else:
                                            print_pass(row)
                                    print()
                                    something_wrong = True
                            else:
                                print_additional_trigger(trigger, len(triggers_in_output[trigger]))
                                something_wrong = True
                        if something_wrong:
                            tests_wrong_out.append(test_name)
                    else:
                        tests_crashed.append(test_name)
                        print_fail("Returned with error code")
                        err_out.append("Returned with error code")
                        something_wrong = True

                if something_wrong:
                    if False:
                        print("STDOUT")
                        print(run_result.stdout)
                        print("STDERR")
                        print(run_result.stderr)
                    print_fail("FAIL")
                    print_fail(test_json["rationale"])
                    return_code = 1
                    if timed_out:
                        test_case = TestCase(test_name, classname=mode)
                        test_case.add_error_info(message="timeout reached")
                        testcases.append(test_case)
                    else:
                        test_case = TestCase(test_name, classname=mode)
                        test_case.add_failure_info(output="\n".join(err_out))
                        testcases.append(test_case)
                else:
                    testcases.append(TestCase(test_name, classname=mode))
                    tests_passed.append(test_name)
                    print_pass("PASS")

                print("")
    print("========================================================================")
    print("Total tests: {}".format(total_number_of_tests))
    print_pass("Tests passed: {}".format(len(tests_passed)))
    if len(tests_crashed) > 0:
        print_fail("Tests crashed: {}".format(len(tests_crashed)))
        for test in tests_crashed:
            print_fail("\t{}".format(test))
    if len(tests_wrong_out) > 0:
        print_fail("Tests with wrong output: {}".format(len(tests_wrong_out)))
        for test in tests_wrong_out:
            print_fail("\t{}".format(test))
    if total_number_of_tests > 0:
        print("")
        print_bold("Passing rate: {:.2f}%".format((100.0*len(tests_passed)/total_number_of_tests)))
    print("========================================================================")
    testsuite = TestSuite("E2E tests", testcases)
    TestSuite.to_file(results_file, [testsuite], prettyprint=True)
sys.exit(return_code)
