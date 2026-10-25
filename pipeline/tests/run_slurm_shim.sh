#!/usr/bin/env bash
# Runs the stub workflow with the slurm executor against fake sbatch, squeue and scancel
# (pipeline/tests/slurm-shim/) that run each job on this machine, so Nextflow's SLURM calls,
# its parsing of the job id, the routing of tasks to partitions, the time limit GNOMAD_SITES
# asks for and its detection of a finished job are checked without a cluster.
#
#   bash pipeline/tests/run_slurm_shim.sh
#
# Needs nextflow on PATH. Offline: -stub runs no real work, -profile host uses no container.
# Everything it writes goes to a temporary directory that is removed at the end, and every
# process it starts is gone when it returns. SLURM_SHIM_TIMEOUT (seconds, default 900) stops
# a run that hangs.
#
# It is not an nf-test file, so a plain `nf-test test` does not run it. The real cluster is
# not tested by this: the shims follow what Nextflow 26.04.7 calls, not what a site's SLURM
# does.
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
pipeline=$(dirname "$here")
limit=${SLURM_SHIM_TIMEOUT:-900}

fail() {
    printf 'FAIL: %s\n' "$*" >&2
    exit 1
}

for tool in sbatch squeue scancel; do
    [[ -x $here/slurm-shim/$tool ]] || fail "$here/slurm-shim/$tool is missing or not executable"
done
command -v nextflow > /dev/null || fail "nextflow is not on PATH"

scratch=$(mktemp -d)
export FAKE_SLURM_DIR=$scratch/fake-slurm      # the shims' job table
export NXF_DISABLE_CHECK_LATEST=true           # no update check: this test needs no network
export PATH="$here/slurm-shim:$PATH"
nf_pid=''
dog_pid=''

# Stops what is still running and removes the scratch directory, whichever way the script ends.
# Nextflow and the watchdog each run in their own process group (set -m below), and so does
# every job the shims start (pgid = the pid in its job directory). Nextflow gets time to
# cancel its jobs and write its logs before the directory goes, so nothing writes into it
# while it is removed.
cleanup() {
    local status=$? i pid_file
    trap - EXIT INT TERM
    if [[ -n $dog_pid ]]; then
        kill -s KILL -- "-$dog_pid" 2> /dev/null || true
        wait "$dog_pid" 2> /dev/null || true
    fi
    if [[ -n $nf_pid ]]; then
        kill -s TERM -- "-$nf_pid" 2> /dev/null || true
        for i in $(seq 20); do
            kill -0 "$nf_pid" 2> /dev/null || break
            sleep 1
        done
        kill -s KILL -- "-$nf_pid" 2> /dev/null || true
        wait "$nf_pid" 2> /dev/null || true
    fi
    for pid_file in "$FAKE_SLURM_DIR"/jobs/*/pid; do
        [[ -f $pid_file ]] && kill -s KILL -- "-$(cat "$pid_file")" 2> /dev/null || true
    done
    rm -rf "$scratch"
    exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

# Run in the scratch directory, so .nextflow.log and .nextflow/ land there, with stdin closed
# (a background process group that reads the terminal is stopped).
set -m
(
    cd "$scratch"
    exec nextflow run "$pipeline" -profile slurm,host -c "$here/slurm.config" -stub -ansi-log false \
        -work-dir "$scratch/work" \
        --chrom MT --datasets clinvar,gnomad_genomes --slurm_queue normal --network_queue transfer \
        --outdir "$scratch/out"
) < /dev/null > "$scratch/nextflow.out" 2>&1 &
nf_pid=$!
(
    sleep "$limit"
    : > "$scratch/timed-out"
    kill -s TERM -- "-$nf_pid"
) < /dev/null > /dev/null 2>&1 &
dog_pid=$!
set +m

status=0
wait "$nf_pid" || status=$?
kill -s KILL -- "-$dog_pid" 2> /dev/null || true
wait "$dog_pid" 2> /dev/null || true
dog_pid=''
nf_pid=''

if [[ -e $scratch/timed-out ]]; then
    tail -n 30 "$scratch/nextflow.out" >&2
    fail "nextflow run did not finish in ${limit}s (SLURM_SHIM_TIMEOUT)"
fi
if ((status != 0)); then
    tail -n 40 "$scratch/nextflow.out" >&2
    fail "nextflow run exited with status $status"
fi
echo "ok   nextflow run -profile slurm,host -stub succeeded"

# Every task is in the trace with the job id the sbatch shim printed.
trace=$scratch/out/pipeline_info/trace.txt
[[ -s $trace ]] || fail "no trace at pipeline_info/trace.txt"
trace_col() { awk -F'\t' -v name="$1" 'NR == 1 { for (i = 1; i <= NF; i++) if ($i == name) print i }' "$trace"; }
native_col=$(trace_col native_id)
hash_col=$(trace_col hash)
name_col=$(trace_col name)
status_col=$(trace_col status)
[[ -n $native_col && -n $hash_col && -n $name_col && -n $status_col ]] ||
    fail "trace.txt has no native_id, hash, name or status column: $(head -n 1 "$trace")"
tasks=$(awk 'END { print NR - 1 }' "$trace")
((tasks > 0)) || fail "trace.txt lists no task"

bad=$(awk -F'\t' -v c="$native_col" -v n="$name_col" 'NR > 1 && $c !~ /^[0-9]+$/ { print "  " $n ": native_id \"" $c "\"" }' "$trace")
[[ -z $bad ]] || fail "trace rows without a numeric native_id:"$'\n'"$bad"
echo "ok   all $tasks trace rows have a numeric native_id"

bad=$(awk -F'\t' -v c="$status_col" -v n="$name_col" 'NR > 1 && $c != "COMPLETED" { print "  " $n ": " $c }' "$trace")
[[ -z $bad ]] || fail "tasks that did not complete:"$'\n'"$bad"
echo "ok   all $tasks tasks COMPLETED"

# Every task went through sbatch, and squeue reports each of those jobs as completed.
njobs=$(find "$FAKE_SLURM_DIR/jobs" -mindepth 1 -maxdepth 1 -type d | awk 'END { print NR }')
((njobs == tasks)) || fail "$tasks tasks but $njobs jobs reached the sbatch shim"
bad=$(squeue --noheader -o '%i %t' -t all | grep -Ev '^[0-9]+ CD$' || true)
[[ -z $bad ]] || fail "squeue shows jobs that are not CD:"$'\n'"$bad"
echo "ok   sbatch took $njobs jobs and squeue shows every one CD"

# A task's partition is the line `#SBATCH -p <queue>` in its .command.run: --network_queue for
# a task labelled network, --slurm_queue for every other. The labels are read from the
# modules, so a process that gains or loses 'network' is checked against its new label.
processes_with_label() {
    awk -v label="$1" '
        /^process[ \t]+[A-Za-z0-9_]+/ { proc = $2 }
        $0 ~ "^[ \t]*label[ \t]+.?" label ".?[ \t]*$" { print proc }
    ' "$pipeline"/modules/*.nf
}
network=$(processes_with_label network)
sorted=$(processes_with_label sort)
[[ -n $network && -n $sorted ]] || fail "found no process labelled network or sort in $pipeline/modules"

seen_network=0
seen_sort=0
seen_gnomad=0
while IFS=$'\t' read -r hash name; do
    process=${name%% *}      # "REGENERATE:FETCH_FASTA (MT)" is the task name
    process=${process##*:}
    dirs=("$scratch/work/$hash"*)
    [[ -d ${dirs[0]} ]] || fail "no work directory for task $name ($hash)"
    run_file=${dirs[0]}/.command.run
    [[ -f $run_file ]] || fail "$name: no .command.run in ${dirs[0]}"
    headers=$(grep -E '^#SBATCH -p ' "$run_file" || true)
    if grep -qxF "$process" <<< "$network"; then
        expected='#SBATCH -p transfer'
        seen_network=$((seen_network + 1))
    else
        expected='#SBATCH -p normal'
        grep -qxF "$process" <<< "$sorted" && seen_sort=$((seen_sort + 1))
    fi
    [[ $headers == "$expected" ]] ||
        fail "$name: expected the header \`$expected\` in .command.run, found: ${headers:-none}"
    # Only GNOMAD_SITES asks for a time limit (conf/slurm.config); the rest take the partition's.
    times=$(grep -E '^#SBATCH -t ' "$run_file" || true)
    expected=''
    if [[ $process == GNOMAD_SITES ]]; then
        expected='#SBATCH -t 24:00:00'
        seen_gnomad=$((seen_gnomad + 1))
    fi
    if [[ $times != "$expected" ]]; then
        want=${expected:+the header \`$expected\`}
        fail "$name: expected ${want:-no #SBATCH -t header} in .command.run, found: ${times:-none}"
    fi
done < <(awk -F'\t' -v h="$hash_col" -v n="$name_col" 'NR > 1 { print $h "\t" $n }' "$trace")
((seen_network > 0)) || fail "no task labelled network ran, so --network_queue was not exercised"
((seen_sort > 0)) || fail "no task labelled sort ran, so --slurm_queue was not exercised"
((seen_gnomad > 0)) || fail "no GNOMAD_SITES task ran, so its time limit was not exercised"
echo "ok   $seen_network network tasks have #SBATCH -p transfer, every other task (sort included) -p normal"
echo "ok   $seen_gnomad GNOMAD_SITES task(s) have #SBATCH -t 24:00:00, no other task has a time limit"

# No job outlives the run.
for pid_file in "$FAKE_SLURM_DIR"/jobs/*/pid; do
    if kill -0 -- "-$(cat "$pid_file")" 2> /dev/null; then
        fail "a job process is still running: $(dirname "$pid_file")"
    fi
done
echo "ok   no job process is left"

echo "slurm shim test passed"
