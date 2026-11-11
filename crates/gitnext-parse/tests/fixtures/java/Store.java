package com.acme.app;

import java.util.List;
import org.springframework.web.bind.annotation.GetMapping;
import static org.junit.Assert.assertTrue;

/** Keeps tasks. */
public class Store extends Base implements Saver {
    private List<Task> items;
    /** Make one. */
    public Store(String path) { super(path); }
    @Override
    public int save(boolean force) throws IOException { this.flush(); helper(); Util.fmt(items); return items.size(); }
    interface Inner { void run(); }
    enum Color { RED, GREEN }
}
record Point(int x, int y) {}
class StoreTest { @Test void saves() { assertTrue(new Store("a").save(true) > 0); } }
