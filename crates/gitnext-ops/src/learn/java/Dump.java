package gitnextlearn;

import java.io.*;
import java.nio.file.*;

/** The JaCoCo agent's data of every test on its own: taken, and reset, when a test starts and when it ends. */
public final class Dump {
    static final Path DIR = Paths.get(System.getProperty("gitnext.dir", "gitnext-exec"));
    static final Object agent;
    static final java.lang.reflect.Method get;
    static int seq;
    static PrintWriter index;
    static {
        try {
            agent = Class.forName("org.jacoco.agent.rt.RT").getMethod("getAgent").invoke(null);
            get = Class.forName("org.jacoco.agent.rt.IAgent").getMethod("getExecutionData", boolean.class);
            Files.createDirectories(DIR);
            index = new PrintWriter(new FileWriter(DIR.resolve("index.tsv").toFile(), true), true);
        } catch (Exception e) {
            throw new ExceptionInInitializerError(e);
        }
    }

    static byte[] take() {
        try {
            return (byte[]) get.invoke(agent, true);
        } catch (Exception e) {
            throw new RuntimeException(e);
        }
    }

    // several test JVMs write to the same directory: the pid keeps their files apart
    static final String JVM = Long.toString(ProcessHandle.current().pid());

    static void write(String cls, String method, boolean failed, byte[] data) {
        String n = JVM + "-" + seq++;
        try {
            Files.write(DIR.resolve(n + ".exec"), data);
        } catch (IOException e) {
            throw new UncheckedIOException(e);
        }
        index.println(n + "\t" + cls + "\t" + method + "\t" + (failed ? 1 : 0));
    }

    /** A test class starts: what ran before it is no test's. Its static blocks run now, not in its first
     *  test: they are all of its tests'. */
    public static synchronized void classStarted(String cls) {
        take();
        try {
            Class.forName(cls, true, Thread.currentThread().getContextClassLoader());
        } catch (Throwable e) {
            // its tests fail on their own
        }
    }

    /** What ran since the class started or since its last test (`@BeforeAll`, `@AfterAll`, a static block):
     *  all of its tests'. */
    public static synchronized void classPart(String cls) {
        write(cls, "", false, take());
    }

    public static synchronized void started(String cls) {
        classPart(cls);
    }

    public static synchronized void finished(String cls, String method, boolean failed) {
        write(cls, method, failed, take());
    }
}
