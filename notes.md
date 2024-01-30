# 1BRC in Rust solution
Using 32vCPU;128GB x86 instance on AWS (m6a.8xlarge).

Baseline takes about 165 sec

## Iteration 1
- Use single thread to perform all of the operations.
- Read all the file in the memory and then process it.
- Use standard library for splitting the line and parsing them into `<location, data>`.

Total time taken: 86 sec

### Observations
![First flamegraph](./assets/flamegraph.1.svg)

- Reading the entire file into program memory and then iterating over it line by line seems to be taking about 30% of time. It might be better to read it line by line first.

## Iteration 2
- Use single thread to perform all of the operations.
- Read the file line by line but store all the lines in memory as read.
- Use standard library for splitting the line and parsing them into `<location, data>`.

Total time taken: 85 sec

### Observations
![Second flamegraph](./assets/flamegraph.2.svg)

- Because the file is already in a RAMDisk so reading all at once vs reading line by line does not seem to be making any difference.
- It seems that iterating over the file this way isn't really cutting it.