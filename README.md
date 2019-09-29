# Minitarp

A minimal version of tarpaulin for testing. This just has the process handling
code in. Statistics gathering, binary parsing, source analysis and report
generation are removed. Instead provide a toml file following the format of
`minitarp.toml` in the repo root and a path to a binary. 

To populate `minitarp.toml` you can get the points to instrument by running 
`cargo tarpaulin --debug` and any other options which affect instrumentation.
The path to the binary will be printed by tarpaulin before it starts the run.

You can then parse the printouts, but alternatively I'm working on a gnuplot
for debugging.
