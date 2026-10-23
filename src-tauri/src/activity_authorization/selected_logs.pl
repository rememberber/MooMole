use strict;
use warnings;
use JSON::PP;
use Encode qw(encode);
use IO::Select;
use POSIX qw(WNOHANG);
use Time::HiRes qw(clock_gettime CLOCK_MONOTONIC);
$| = 1;
sub mono { return int(clock_gettime(CLOCK_MONOTONIC) * 1000); }
sub reject { exit 126; }
my $hex = shift @ARGV;
defined($hex) && $hex =~ /\A[0-9a-f]{2,49152}\z/ && length($hex) % 2 == 0 && !@ARGV or reject();
my $request = eval { JSON::PP->new->utf8->decode(pack('H*', $hex)) };
ref($request) eq 'HASH' or reject();
my ($nonce, $digest, $uid, $home, $expires) = @{$request}{qw(nonce digest uid home expires_ms)};
defined($nonce) && $nonce =~ /\A[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}\z/ or reject();
defined($digest) && $digest =~ /\A[0-9a-f]{64}\z/ or reject();
defined($uid) && $uid =~ /\A[1-9][0-9]{0,9}\z/ && $uid <= 4294967295 && $> == 0 or reject();
defined($home) && index($home, "\0") == -1 && $home =~ m{\A/[^/]} && $home !~ m{/(?:\.|\.\.|)(?:/|\z)} or reject();
my @account = getpwuid($uid);
@account && $account[7] eq encode('UTF-8', $home) or reject();
defined($expires) && $expires =~ /\A[0-9]{1,16}\z/ or reject();
my $now = mono();
$expires > $now && $expires <= $now + 120000 or reject();
ref($request->{items}) eq 'ARRAY' && @{$request->{items}} >= 1 && @{$request->{items}} <= 20 or reject();
my %seen;
for my $item (@{$request->{items}}) {
    ref($item) eq 'HASH' && defined($item->{id}) && length($item->{id}) <= 128 && !$seen{$item->{id}}++ or reject();
    my $path = $item->{path};
    defined($path) && index($path, "\0") == -1 && length(encode('UTF-8', $path)) <= 4096 or reject();
    my $prefix = $home . '/Library/Logs/';
    index($path, $prefix) == 0 or reject();
    my $tail = substr($path, length($prefix));
    $tail =~ s{\ADiagnosticReports/}{};
    length($tail) && $tail !~ m{[\/]} && $tail !~ /\A\./ or reject();
    my @parts = split('/', $path, -1);
    shift @parts;
    @parts < 128 && !grep { !length($_) || $_ eq '.' || $_ eq '..' } @parts or reject();
    my @paths = ('/');
    my $current = '';
    for my $part (@parts) { $current .= '/' . $part; push @paths, $current; }
    ref($item->{nodes}) eq 'ARRAY' && @{$item->{nodes}} == @paths or reject();
    for my $index (0..$#paths) {
        my $node = $item->{nodes}[$index];
        ref($node) eq 'ARRAY' && @$node == 6 && $node->[0] eq $paths[$index] or reject();
        for my $value (@$node[1..5]) { defined($value) && $value =~ /\A[0-9]{1,20}\z/ or reject(); }
        my $kind = $node->[3] & 0170000;
        $kind == ($index == $#paths ? 0100000 : 0040000) or reject();
    }
    $item->{nodes}[-1][4] eq "$uid" or reject();
    for my $key (qw(size links)) { defined($item->{$key}) && $item->{$key} =~ /\A[0-9]{1,20}\z/ or reject(); }
    $item->{links} eq '1' or reject();
    for my $key (qw(mtime ctime)) { defined($item->{$key}) && $item->{$key} =~ /\A-?[0-9]{1,19}\z/ or reject(); }
}
open STDIN, '<', '/dev/null' or reject();
# A shared five-second budget covers visibility AND all exact-file observations.
# Child ownership is retained until reaping; no asynchronous PID watchdog.
my $deadline = mono() + 5000;
$deadline = $expires if $expires < $deadline;
sub query {
    my ($mode, $path) = @_;
    return (124, '') if mono() >= $deadline;
    pipe(my $reader, my $writer) or return (125, '');
    my $child = fork();
    defined($child) or return (125, '');
    if (!$child) {
        close $reader;
        open STDOUT, '>&', $writer or exit 126;
        open STDERR, '>&', $writer or exit 126;
        close $writer;
        if ($mode eq 'visibility') {
            exec { '/usr/sbin/lsof' } '/usr/sbin/lsof', '-F', 'pu', '-p', '1';
        } else {
            exec { '/usr/sbin/lsof' } '/usr/sbin/lsof', '-F', 'pfn', '--', encode('UTF-8', $path);
        }
        exit 126;
    }
    close $writer;
    my $selector = IO::Select->new($reader);
    my ($records, $reaped, $status, $eof, $failure) = ('', 0, 0, 0, 0);
    while (1) {
        if (!$eof && $selector->can_read(0.02)) {
            my $read = sysread($reader, my $chunk, 4096);
            if (!defined($read)) { $failure = 125; last; }
            if ($read == 0) { $eof = 1; }
            else { $records .= $chunk; }
            if (length($records) > 4096) { $records = ''; $failure = 125; last; }
        }
        if (!$reaped) {
            my $waited = waitpid($child, WNOHANG);
            if ($waited == $child) { $status = $?; $reaped = 1; }
            elsif ($waited == -1) { $reaped = 1; $failure = 125; last; }
        }
        last if $eof && $reaped;
        if (mono() >= $deadline) { $failure = 124; last; }
        select(undef, undef, undef, 0.02) if $eof;
    }
    if (!$reaped) { kill 'KILL', $child; waitpid($child, 0); }
    close $reader;
    return ($failure, '') if $failure;
    return (125, '') if $status & 127;
    return ($status >> 8, $records);
}
sub same_identity {
    my ($item) = @_;
    for my $node (@{$item->{nodes}}) {
        my @stat = lstat(encode('UTF-8', $node->[0]));
        @stat or return 0;
        for my $pair ([0,1], [1,2], [2,3], [4,4], [5,5]) {
            "$stat[$pair->[0]]" eq $node->[$pair->[1]] or return 0;
        }
    }
    my @stat = lstat(encode('UTF-8', $item->{path}));
    @stat or return 0;
    return "$stat[7]" eq $item->{size} && "$stat[3]" eq $item->{links}
        && "$stat[9]" eq $item->{mtime} && "$stat[10]" eq $item->{ctime};
}
my ($status, $records) = query('visibility', '');
my $visible = $status == 0 && $records =~ /\Ap1\nu0\n(?:f(?:[0-9]{1,10}|cwd|rtd|txt|mem|mmap|pd)\n)*\z/;
$visible = 0 if grep { /\Af([0-9]+)\z/ && $1 > 4294967295 } split("\n", $records);
my @results;
for my $item (@{$request->{items}}) {
    my $at = mono();
    my $code = 'unknown';
    if ($at >= $deadline) { $code = 'budgetExhausted'; }
    elsif (!same_identity($item)) { $code = 'changed'; }
    elsif ($visible) {
        my ($exit, $output) = query('exact', $item->{path});
        $code = $exit == 0 ? 'busy' : ($exit == 1 && $output eq '' ? 'noOpenHandles' : 'unknown');
        $code = 'budgetExhausted' if $exit == 124;
        $code = 'changed' if !same_identity($item);
    }
    push @results, { id => $item->{id}, code => $code, checked_ms => $at };
}
print JSON::PP->new->canonical->utf8->encode({ version => 1, nonce => $nonce, digest => $digest,
    uid => 0 + $>, visibility => $visible ? 'observed' : 'unknown', items => \@results, complete => 'MooMole.logs.complete.v1' });
