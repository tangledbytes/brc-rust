# 1BRC in Rust solution
Using 32vCPU;128GB x86 instance on AWS (m6a.8xlarge).

Baseline takes about 165 sec

## Iteration 0
- Use single thread to perform all of the operations.
- Read all the file in the memory and then process it.
- Use standard library for splitting the line and parsing them into `<location, data>`.

Total time take: 86 sec

### Observations
![First flamegraph](./assets/flamegraph.1.svg)

- Reading the entire file into program memory and then iterating over it line by line seems to be taking about 30% of time. It might be better to read it line by line first.