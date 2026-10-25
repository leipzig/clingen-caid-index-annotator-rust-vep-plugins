import groovy.json.JsonOutput

/**
 * A stand-in for the ClinGen Allele Registry on 127.0.0.1, so that the offline tests can run
 * ENUMERATE, LOOKUP and the whole workflow without the network (nf-test reads this from
 * tests/lib, its default libDir). It answers the two requests clingen-index makes, with only
 * the fields clingen-index asks for:
 *
 *   GET  /alleles?refseq=R&begin=B&end=E&...   every allele given whose HGVS on R starts in (B, E]
 *   POST /alleles?file=vcf&...                 one answer per VCF data line, from the rows given
 *
 * and records each request line. Use it as a Closeable: close() stops it.
 */
class MockRegistry implements Closeable {

    /** Request lines, e.g. 'GET /alleles?refseq=NC_012920.1&begin=2980&end=4600&...'. */
    final List<String> requests = Collections.synchronizedList([])

    private final ServerSocket server = new ServerSocket(0, 50, InetAddress.getLoopbackAddress())
    private final List<List<String>> alleles
    private final Map<String, List<String>> answers

    /**
     * @param alleles     [HGVS, CAID] of the alleles the region query knows
     * @param harvestRows the 7 harvest columns (CHROM POS REF ALT CAID HGVS_GRCh38 HGVS_GRCh37) a
     *                    lookup answers with; an empty CAID is an allele nobody registered
     */
    MockRegistry(List<List<String>> alleles, List<List<String>> harvestRows) {
        this.alleles = alleles
        this.answers = harvestRows.collectEntries { row -> [row[0..3].join('\t'), row[4..6]] }
        Thread.startDaemon('mock-registry') { serve() }
    }

    /** The --base-url / --registry_url to give clingen-index. */
    String getUrl() {
        return "http://127.0.0.1:${server.localPort}".toString()
    }

    void close() {
        server.close()
    }

    private void serve() {
        while (!server.closed) {
            Socket socket
            try {
                socket = server.accept()
            }
            catch (SocketException ignored) {
                return   // closed
            }
            socket.withCloseable { handle(it) }
        }
    }

    private void handle(Socket socket) {
        def input = new BufferedInputStream(socket.inputStream)
        def requestLine = readLine(input)
        int length = 0
        for (String header = readLine(input); header; header = readLine(input)) {
            if (header.toLowerCase().startsWith('content-length:')) {
                length = header.substring('content-length:'.length()).trim() as int
            }
        }
        def body = new String(input.readNBytes(length), 'UTF-8')
        requests << requestLine

        def (method, target) = requestLine.tokenize(' ')
        def query = target.contains('?') ? target.substring(target.indexOf('?') + 1).tokenize('&').collectEntries { p ->
            def (k, v) = (p.tokenize('=') + [''])[0..1]
            [k, URLDecoder.decode(v, 'UTF-8')]
        } : [:]
        def answer = method == 'GET' ? region(query.refseq, query.begin as long, query.end as long) : lookup(body)
        def bytes = JsonOutput.toJson(answer).getBytes('UTF-8')
        def out = socket.outputStream
        out << "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: ${bytes.length}\r\nConnection: close\r\n\r\n"
        out << bytes
        out.flush()
    }

    private List region(String refseq, long begin, long end) {
        return alleles.findAll { hgvs, ca ->
            def m = hgvs =~ /^${java.util.regex.Pattern.quote(refseq)}:[gm]\.(\d+)/
            m.find() && (m.group(1) as long) > begin && (m.group(1) as long) <= end
        }.collect { hgvs, ca -> allele("http://reg.genome.network/allele/${ca}", hgvs, '') }
    }

    private List lookup(String vcf) {
        return vcf.readLines().findAll { line -> line && !line.startsWith('#') }.collect { line ->
            def (chrom, pos, id, ref, alt) = line.split('\t') as List
            def row = answers["${chrom}\t${pos}\t${ref}\t${alt}".toString()]
            if (row == null) {
                return [errorType: 'NotInMockRegistry', inputLine: line]
            }
            def (ca, grch38, grch37) = row
            allele(ca ? "http://reg.genome.network/allele/${ca}" : '_:CA', grch38, grch37)
        }
    }

    private static Map allele(String id, String grch38, String grch37) {
        def genomic = [[hgvs: [grch38], referenceGenome: 'GRCh38']]
        if (grch37) {
            genomic << [hgvs: [grch37], referenceGenome: 'GRCh37']
        }
        return ['@id': id.toString(), genomicAlleles: genomic]
    }

    // A header line without its CR LF; '' at the blank line that ends the headers.
    private static String readLine(InputStream input) {
        final int CR = 13, LF = 10
        def line = new ByteArrayOutputStream()
        for (int b = input.read(); b != -1 && b != LF; b = input.read()) {
            if (b != CR) {
                line.write(b)
            }
        }
        return line.toString('UTF-8')
    }
}
