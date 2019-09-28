import os

failures = 0
tests = 1000
for i in range(tests):
    ret = os.system('cargo run &> /dev/null')
    if ret is not 0 :
        failures += 1

print("Failed {}/{} times".format(failures, tests))
